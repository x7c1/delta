//! History aggregations over the session rows: the recent-workdir list, the
//! "has this cwd ever been seen" check, and the repository clone rows. Each
//! mirrors its `SqliteStore` twin in `session_history`.

use crate::error::Result;
use crate::ports::RepositoryCloneRow;

use super::FakeStore;

impl FakeStore {
    pub(super) async fn recent_workdirs(
        &self,
        limit: u32,
    ) -> Result<Vec<crate::ports::RecentWorkdir>> {
        let g = self.inner.lock().unwrap();
        // Mirror the SQLite query: group on `COALESCE(requested_workdir, cwd)`
        // so worktree-managed paths drop out and legacy rows still surface by
        // their `cwd`. Per-session recency is the latest message, else the
        // session's `created_at`; a workdir's recency is the max across its
        // sessions.
        let mut by_workdir: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for s in &g.sessions {
            let recency = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max()
                .unwrap_or_else(|| s.created_at.clone());
            let workdir = s.requested_workdir.clone().unwrap_or_else(|| s.cwd.clone());
            by_workdir
                .entry(workdir)
                .and_modify(|cur| {
                    if recency > *cur {
                        *cur = recency.clone();
                    }
                })
                .or_insert(recency);
        }
        let mut rows: Vec<(String, Option<String>)> = by_workdir
            .into_iter()
            .map(|(workdir, recency)| (workdir, Some(recency)))
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit as usize);
        Ok(rows)
    }

    pub(super) async fn cwd_exists(&self, path: &str) -> Result<bool> {
        // Mirror the SQLite UNION: match any session.cwd, session.requested_workdir,
        // or message.cwd equal to `path` (byte-for-byte).
        let g = self.inner.lock().unwrap();
        let hit_in_sessions = g
            .sessions
            .iter()
            .any(|s| s.cwd == path || s.requested_workdir.as_deref() == Some(path));
        if hit_in_sessions {
            return Ok(true);
        }
        let hit_in_messages = g.messages.iter().any(|m| m.cwd.as_deref() == Some(path));
        Ok(hit_in_messages)
    }

    pub(super) async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>> {
        let g = self.inner.lock().unwrap();
        // Mirror the SQL pipeline in pure Rust:
        //
        // 1. Group all sessions by `(repo_root, clone_path)` and keep the
        //    newest row per group. Recency is `last_activity_at` (the latest
        //    message's `created_at`) when set, else the session's `created_at`;
        //    ties break on insertion order (the fake's proxy for the real
        //    store's monotonic `id` DESC tie-break).
        // 2. Compute each `repo_root`'s recency as the max group recency, then
        //    take the top `active_repo_limit` by `(recency DESC, repo_root
        //    ASC)`.
        // 3. Within each retained `repo_root`, classify each row's kind by the
        //    `worktree_base + "/"` prefix and cap user/generated paths
        //    separately.
        // 4. Sort by `(recency DESC, repo_root ASC, clone_path ASC)`, matching
        //    the SQL ORDER BY.
        struct Acc {
            latest_recency: Option<String>,
            latest_branch: Option<String>,
            latest_index: i64,
        }
        let mut by_pair: std::collections::HashMap<(String, String), Acc> =
            std::collections::HashMap::new();
        for (index, s) in g.sessions.iter().enumerate() {
            let index = index as i64;
            let Some(repo_root) = &s.repo_root else {
                continue;
            };
            let clone_path = s.requested_workdir.clone().unwrap_or_else(|| s.cwd.clone());
            let recency = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max()
                .or_else(|| Some(s.created_at.clone()));
            let key = (repo_root.clone(), clone_path);
            let entry = by_pair.entry(key).or_insert_with(|| Acc {
                latest_recency: recency.clone(),
                latest_branch: s.branch_at_launch.clone(),
                latest_index: index,
            });
            let beats = match (&recency, &entry.latest_recency) {
                (Some(a), Some(b)) if a > b => true,
                (Some(_), None) => true,
                (Some(a), Some(b)) if a == b && index > entry.latest_index => true,
                _ => false,
            };
            if beats {
                entry.latest_recency = recency;
                entry.latest_branch = s.branch_at_launch.clone();
                entry.latest_index = index;
            }
        }

        // The `latest` row set: one per `(repo_root, clone_path)`.
        let latest: Vec<RepositoryCloneRow> = by_pair
            .into_iter()
            .map(|((repo_root, clone_path), acc)| RepositoryCloneRow {
                repo_root,
                clone_path,
                last_opened_at: acc.latest_recency,
                last_branch: acc.latest_branch,
            })
            .collect();

        // Active repo selection: sort repo_roots by (max recency DESC,
        // repo_root ASC), keep the top `active_repo_limit`.
        let mut by_root: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        for row in &latest {
            let entry = by_root.entry(row.repo_root.clone()).or_insert(None);
            if row.last_opened_at > *entry {
                *entry = row.last_opened_at.clone();
            }
        }
        let mut root_recencies: Vec<(String, Option<String>)> = by_root.into_iter().collect();
        root_recencies.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let active_roots: std::collections::HashSet<String> = root_recencies
            .into_iter()
            .take(active_repo_limit.max(0) as usize)
            .map(|(root, _)| root)
            .collect();

        // Drop rows whose repo_root did not survive the active-repo cap, then
        // partition each surviving repo_root's rows by kind (`worktree_base +
        // "/"` prefix), sort each kind by (recency DESC, clone_path ASC), and
        // take its respective cap.
        let prefix = format!("{worktree_base}/");
        let mut surviving: Vec<RepositoryCloneRow> = latest
            .into_iter()
            .filter(|row| active_roots.contains(&row.repo_root))
            .collect();
        // Group by repo_root.
        let mut grouped: std::collections::HashMap<String, Vec<RepositoryCloneRow>> =
            std::collections::HashMap::new();
        for row in surviving.drain(..) {
            grouped.entry(row.repo_root.clone()).or_default().push(row);
        }
        let mut out: Vec<RepositoryCloneRow> = Vec::new();
        for (_, rows) in grouped {
            let (mut generated, mut user): (Vec<_>, Vec<_>) = rows
                .into_iter()
                .partition(|r| r.clone_path.starts_with(&prefix));
            let sort_rows = |v: &mut Vec<RepositoryCloneRow>| {
                v.sort_by(|a, b| {
                    b.last_opened_at
                        .cmp(&a.last_opened_at)
                        .then_with(|| a.clone_path.cmp(&b.clone_path))
                });
            };
            sort_rows(&mut user);
            sort_rows(&mut generated);
            user.truncate(user_clone_limit.max(0) as usize);
            generated.truncate(generated_clone_limit.max(0) as usize);
            out.extend(user);
            out.extend(generated);
        }

        // Final ORDER BY: (recency DESC, repo_root ASC, clone_path ASC).
        out.sort_by(|a, b| {
            b.last_opened_at
                .cmp(&a.last_opened_at)
                .then_with(|| a.repo_root.cmp(&b.repo_root))
                .then_with(|| a.clone_path.cmp(&b.clone_path))
        });
        Ok(out)
    }
}
