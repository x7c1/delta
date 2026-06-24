//! `list_repositories`: build the recency-ordered Repository tab list.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::repository::{display_name, identity_key, Clone as RepoClone, Repository};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Aggregate the session history into the Repository tab's view: every
    /// repository the user has launched a session under, ordered by the most
    /// recent activity across its clones.
    ///
    /// Each `(repo_root, clone_path)` pair from
    /// [`SessionStore::repository_clone_rows`] is mapped to a Repository via
    /// `git config --get remote.origin.url` on the `repo_root`
    /// ([`GitWorktree::origin_url`], cached for the process's lifetime so the
    /// same root is not re-shelled out per call). Clones with the same
    /// `identity_key` bundle into one repository; clones whose path no longer
    /// exists are filtered out (lazy GC), and a repository emptied by that
    /// drops from the result.
    pub async fn list_repositories(&self) -> Result<Vec<Repository>> {
        let rows = self.store.repository_clone_rows().await?;

        // Group raw rows by repo_root, then resolve each root's identity_key
        // once (the `origin_url` lookup is the cost we want to amortise).
        let mut by_root: BTreeMap<String, Vec<_>> = BTreeMap::new();
        for row in rows {
            by_root.entry(row.repo_root.clone()).or_default().push(row);
        }

        // For each repo_root, resolve the identity_key — preferring an
        // origin URL but falling back to the path so a non-origin clone
        // still stands alone in the list. Cache the origin lookup per process.
        let mut by_identity: BTreeMap<String, RepoAcc> = BTreeMap::new();
        for (repo_root, rows) in by_root {
            let origin = self.cached_origin_url(&repo_root).await?;
            let key = identity_key(origin, &repo_root);
            let entry = by_identity.entry(key.clone()).or_insert_with(|| RepoAcc {
                identity_key: key,
                latest_path: String::new(),
                latest_opened_at: None,
                clones: Vec::new(),
            });
            for row in rows {
                // Lazy GC: drop clones whose path no longer exists on disk so a
                // moved or deleted dir simply disappears from the picker. The
                // stat is per-call (no cache) because the underlying path may
                // appear/disappear between calls and a stale "exists" cache
                // would surface ghosts.
                if tokio::fs::metadata(&row.clone_path).await.is_err() {
                    continue;
                }
                if row.last_opened_at > entry.latest_opened_at {
                    entry.latest_opened_at = row.last_opened_at.clone();
                    entry.latest_path = row.clone_path.clone();
                }
                entry.clones.push(RepoClone {
                    path: row.clone_path,
                    last_opened_at: row.last_opened_at,
                    last_branch: row.last_branch,
                    // Per-session launch-option and worktree-state persistence
                    // is deferred to a follow-up PR; until then every clone
                    // reports empty/false here and the frontend falls back to
                    // its current per-spawn defaults. See the plan doc.
                    last_launch_option_ids: Vec::new(),
                    last_worktree_enabled: false,
                    last_worktree_start_point: None,
                });
            }
        }

        // Project the accumulators into Repository values, dropping any
        // accumulator with no surviving clones (every clone path was stale).
        let mut repositories: Vec<Repository> = by_identity
            .into_values()
            .filter_map(|mut acc| {
                if acc.clones.is_empty() {
                    return None;
                }
                // Clones inside a repository are ordered most-recent first;
                // ties on recency fall back to the clone path so the ordering
                // is deterministic across runs.
                acc.clones.sort_by(|a, b| {
                    b.last_opened_at
                        .cmp(&a.last_opened_at)
                        .then_with(|| a.path.cmp(&b.path))
                });
                Some(Repository {
                    display_name: display_name(&acc.identity_key, &acc.latest_path),
                    identity_key: acc.identity_key,
                    recently_used_clone_path: acc.latest_path,
                    clones: acc.clones,
                })
            })
            .collect();

        // Repositories are ordered by the recency of their most-recent clone
        // (the same key the navigator sorts sessions by). Ties fall back to
        // `identity_key` for determinism.
        repositories.sort_by(|a, b| {
            let a_recency = a.clones.first().and_then(|c| c.last_opened_at.as_ref());
            let b_recency = b.clones.first().and_then(|c| c.last_opened_at.as_ref());
            b_recency
                .cmp(&a_recency)
                .then_with(|| a.identity_key.cmp(&b.identity_key))
        });

        Ok(repositories)
    }

    /// Look up (and memoise) the `origin` URL for a repository root.
    ///
    /// Wrapping [`GitWorktree::origin_url`] in a per-process cache: an origin
    /// URL is a property of the on-disk repo and effectively never changes
    /// for the lifetime of the server, so the cost of shelling out to
    /// `git config` is paid once per root. The cache stores the `Option`
    /// faithfully — repeated lookups of a missing origin are still cheap.
    async fn cached_origin_url(&self, repo_root: &str) -> Result<Option<String>> {
        {
            let cache = self.repository_origin_cache.lock().await;
            if let Some(cached) = cache.get(repo_root) {
                return Ok(cached.clone());
            }
        }
        let url = self.git_worktree.origin_url(repo_root).await?;
        let mut cache = self.repository_origin_cache.lock().await;
        cache.insert(repo_root.to_owned(), url.clone());
        Ok(url)
    }
}

/// Running per-`identity_key` accumulator used by [`list_repositories`].
struct RepoAcc {
    identity_key: String,
    /// The most-recent clone's path (becomes `recently_used_clone_path`).
    latest_path: String,
    /// The recency timestamp of `latest_path`, used to pick the winner.
    latest_opened_at: Option<String>,
    /// Every clone that survived the path-exists filter, in insertion order
    /// (sorted by recency once aggregation finishes).
    clones: Vec<RepoClone>,
}
