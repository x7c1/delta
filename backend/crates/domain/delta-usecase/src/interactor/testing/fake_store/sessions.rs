//! Session rows: registration, activation, listing, and the history-derived
//! workdir and repository aggregations.

use delta_model::{AgentProvider, Session, SessionId, SessionStatus, Thread, ThreadId};

use crate::error::Result;
use crate::ports::{NewSession, RepositoryCloneRow, SessionPageRow, SpawningSession};
use crate::SessionPageCursor;

use super::{FakeStore, FakeStoreInner};

/// A row's recency key: its last activity, falling back to the session's own
/// `created_at` when message-less — the `COALESCE(last_activity_at, created_at)`
/// the SQL queries sort on.
fn row_recency(row: &SessionPageRow) -> String {
    row.1.clone().unwrap_or_else(|| row.0.created_at.clone())
}

/// Every stored session as a `(session, last_activity_at)` row, ordered exactly
/// as the SQL session-list queries order them: recency DESC, `created_at` DESC,
/// `id` DESC. Every row is included, a message-less `spawning` one too (see
/// `SqliteStore::list_sessions_page`).
fn recency_ordered_rows(g: &FakeStoreInner) -> Vec<SessionPageRow> {
    let mut rows: Vec<SessionPageRow> = g
        .sessions
        .iter()
        .map(|s| {
            let last_activity_at = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max();
            (s.clone(), last_activity_at)
        })
        .collect();
    rows.sort_by(|a, b| {
        row_recency(b)
            .cmp(&row_recency(a))
            .then_with(|| b.0.created_at.cmp(&a.0.created_at))
            .then_with(|| b.0.id.as_str().cmp(a.0.id.as_str()))
    });
    rows
}

impl FakeStore {
    pub(super) async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        let mut g = self.inner.lock().unwrap();
        // Insert-if-absent, mirroring the real store's upsert: a re-registration
        // returns the existing session and its `main` thread, except that a
        // still-`spawning` row is activated (status flips, the hook-reported
        // transcript path is filled in).
        if let Some(session) = g.sessions.iter_mut().find(|s| s.id == new.id) {
            if session.status == SessionStatus::Spawning {
                session.status = SessionStatus::Active;
                session.cwd = new.cwd;
                session.transcript_path = Some(new.transcript_path);
            }
            let session = session.clone();
            let main_id = g
                .threads
                .iter()
                .find(|t| t.session_id == new.id && t.title == "main")
                .map(|t| t.id)
                .unwrap();
            return Ok((session, main_id));
        }
        let session = Session {
            id: new.id.clone(),
            cwd: new.cwd,
            transcript_path: Some(new.transcript_path),
            title: None,
            status: SessionStatus::Active,
            created_at: "2026-01-01T00:00:00Z".into(),
            // `register_session` is the external-claude / hook-activation
            // path: it never sees Delta's launch context, so the snapshot
            // stays unknown here. Delta-launched sessions record it via
            // `insert_spawning_session` instead, and that row already exists
            // when this activate path runs.
            branch_at_launch: new.branch_at_launch,
            repo_root: new.repo_root,
            // Same as the snapshot fields above: external-claude sessions have
            // no Delta-known launch dir to record. Worktree dirs cannot appear
            // here because external sessions don't go through worktree spawn.
            requested_workdir: None,
            repository_display_name: new.repository_display_name,
            // The hook-activation path is Claude Code only; a structured
            // provider never enters the store this way.
            provider: AgentProvider::Claude,
            provider_session_id: None,
            provider_thread_id: None,
            pull_request_number: None,
            failure_reason: None,
        };
        g.sessions.push(session.clone());
        g.next_thread_id += 1;
        let main_id = ThreadId(g.next_thread_id);
        g.threads.push(Thread {
            id: main_id,
            session_id: new.id,
            title: "main".into(),
            parent_thread_id: None,
            root_message_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_activity_at: None,
        });
        Ok((session, main_id))
    }

    pub(super) async fn insert_spawning_session(
        &self,
        spawning: SpawningSession<'_>,
    ) -> Result<(Session, ThreadId)> {
        let SpawningSession {
            id,
            cwd,
            branch_at_launch,
            repo_root,
            requested_workdir,
            repository_display_name,
            provider,
            pull_request_number,
        } = spawning;
        let mut g = self.inner.lock().unwrap();
        assert!(
            !g.sessions.iter().any(|s| &s.id == id),
            "insert_spawning_session must not be called for an existing id"
        );
        let session = Session {
            id: id.clone(),
            cwd: cwd.to_owned(),
            transcript_path: None,
            title: None,
            status: SessionStatus::Spawning,
            created_at: "2026-01-01T00:00:00Z".into(),
            branch_at_launch: branch_at_launch.map(str::to_owned),
            repo_root: repo_root.map(str::to_owned),
            requested_workdir: requested_workdir.map(str::to_owned),
            repository_display_name: repository_display_name.map(str::to_owned),
            provider,
            provider_session_id: None,
            provider_thread_id: None,
            pull_request_number,
            failure_reason: None,
        };
        g.sessions.push(session.clone());
        g.next_thread_id += 1;
        let main_id = ThreadId(g.next_thread_id);
        g.threads.push(Thread {
            id: main_id,
            session_id: id.clone(),
            title: "main".into(),
            parent_thread_id: None,
            root_message_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_activity_at: None,
        });
        Ok((session, main_id))
    }

    pub(super) async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g.sessions.iter_mut().find(|s| &s.id == id) {
            session.provider_session_id = provider_session_id.map(str::to_owned);
            session.provider_thread_id = provider_thread_id.map(str::to_owned);
            // Mirror the real store: recording the ids activates a still-spawning
            // row (the structured-provider analogue of first-hook activation).
            if session.status == SessionStatus::Spawning {
                session.status = SessionStatus::Active;
            }
        }
        Ok(())
    }

    pub(super) async fn delete_session(&self, id: &SessionId) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the real store's cascade: every child row goes with the
        // session.
        g.sessions.retain(|s| &s.id != id);
        g.threads.retain(|t| &t.session_id != id);
        g.sends.retain(|s| &s.session_id != id);
        g.messages.retain(|m| &m.session_id != id);
        g.permissions.retain(|p| &p.session_id != id);
        g.transcript_lines_read.remove(id);
        g.subagent_launches.retain(|(sid, _), _| sid != id);
        Ok(())
    }

    pub(super) async fn mark_session_failed(
        &self,
        id: &SessionId,
        reason: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g
            .sessions
            .iter_mut()
            .find(|s| &s.id == id && s.status == SessionStatus::Spawning)
        {
            session.status = SessionStatus::Failed;
            session.failure_reason = reason.map(str::to_owned);
        }
        Ok(())
    }

    pub(super) async fn relocate_transcript(
        &self,
        id: &SessionId,
        transcript_path: &str,
        cwd: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g.sessions.iter_mut().find(|s| &s.id == id) {
            session.transcript_path = Some(transcript_path.to_owned());
            if let Some(cwd) = cwd {
                session.cwd = cwd.to_owned();
            }
        }
        Ok(())
    }

    pub(super) async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        let g = self.inner.lock().unwrap();
        let mut rows = recency_ordered_rows(&g);
        // Apply the cursor: keep only rows strictly after it under the same
        // ordering.
        if let Some(c) = cursor {
            rows.retain(|row| {
                let r = row_recency(row);
                r < c.recency
                    || (r == c.recency
                        && (row.0.created_at < c.created_at
                            || (row.0.created_at == c.created_at
                                && row.0.id.as_str() < c.id.as_str())))
            });
        }
        rows.truncate(limit as usize);
        Ok(rows)
    }

    pub(super) async fn list_sessions_by_ids(
        &self,
        ids: &[SessionId],
    ) -> Result<Vec<SessionPageRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let g = self.inner.lock().unwrap();
        // Same recency order as the page query, filtered to the requested ids;
        // an id with no row simply matches nothing, mirroring the SQL `IN`.
        let mut rows = recency_ordered_rows(&g);
        rows.retain(|row| ids.contains(&row.0.id));
        Ok(rows)
    }

    pub(super) async fn session(&self, id: &SessionId) -> Result<Option<Session>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .sessions
            .iter()
            .find(|s| &s.id == id)
            .cloned())
    }

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
