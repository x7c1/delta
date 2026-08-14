//! `list_repositories`: build the recency-ordered Repository tab list.

use std::collections::{BTreeMap, HashSet};

use crate::error::Result;
use crate::interactor::repository::scan::scan_one_root;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::repository::{display_name, identity_key, Clone as RepoClone, Repository};

/// Maximum number of repositories included in the per-repo cap below.
/// Older repositories are dropped wholesale — they're unlikely to be a
/// useful start point for a new session.
pub(crate) const ACTIVE_REPOSITORY_LIMIT: i64 = 20;

/// Per-repository cap on user-picked clone paths (paths outside the
/// auto-generated worktree base). Keeps the main tree and manual sibling
/// clones reliably reachable even when worktree activity is heavy.
pub(crate) const USER_CLONE_PATH_LIMIT: i64 = 5;

/// Per-repository cap on auto-generated worktree paths (children of
/// `$DELTA_WORKTREE_BASE`). Separate from the user-picked cap so a burst
/// of disposable worktrees cannot squeeze out user-meaningful clones.
pub(crate) const GENERATED_CLONE_PATH_LIMIT: i64 = 10;

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
    /// Two clone sets are unioned by `identity_key`:
    ///
    /// 1. Session-derived: every `(repo_root, clone_path)` pair from
    ///    [`SessionStore::repository_clone_rows`] mapped to a Repository via
    ///    `git config --get remote.origin.url` on the `repo_root`
    ///    ([`GitWorktree::origin_url`], cached for the process's lifetime so
    ///    the same root is not re-shelled out per call). Carries per-clone
    ///    `last_opened_at` / `last_branch` derived from the session history.
    /// 2. Scan-derived: every registered clone root's depth-1 children that
    ///    look like git workspaces (`<child>/.git` exists). Carries no
    ///    per-clone history (`last_opened_at: None`) until the user actually
    ///    launches a session in the path. This is how the umbrella-session
    ///    pattern surfaces sub-repo clones the user has not yet started a
    ///    session in.
    ///
    /// Same `identity_key` from both sets bundles into one repository; a clone
    /// path already present from the session-derived side is not added a
    /// second time from the scan side. Clones whose path no longer exists are
    /// filtered out (lazy GC), and a repository emptied by that drops from the
    /// result.
    pub async fn list_repositories(&self) -> Result<Vec<Repository>> {
        let rows = self
            .store
            .repository_clone_rows(
                self.worktree_base.as_str(),
                ACTIVE_REPOSITORY_LIMIT,
                USER_CLONE_PATH_LIMIT,
                GENERATED_CLONE_PATH_LIMIT,
            )
            .await?;

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
                clone_paths: HashSet::new(),
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
                entry.clone_paths.insert(row.clone_path.clone());
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

        // Scan-derived clones. Each registered clone root is enumerated depth-1;
        // its child clones union into `by_identity` keyed by `origin_url` (when
        // set on the child) or by the child's own path (otherwise — the same
        // identity-key fallback the session-derived path uses). A scan-derived
        // clone never overrides the session-derived `latest_path` /
        // `latest_opened_at` (its recency is `None`); when a path already
        // exists in the bundle from the session side it is skipped entirely
        // (de-dup by clone path).
        let clone_roots = self.store.list_clone_roots().await?;
        for root in clone_roots {
            let scanned = scan_one_root(&root.path).await;
            for clone in scanned {
                let origin = self.cached_origin_url(&clone.path).await?;
                let key = identity_key(origin, &clone.path);
                let entry = by_identity.entry(key.clone()).or_insert_with(|| RepoAcc {
                    identity_key: key,
                    latest_path: clone.path.clone(),
                    latest_opened_at: None,
                    clones: Vec::new(),
                    clone_paths: HashSet::new(),
                });
                if !entry.clone_paths.insert(clone.path.clone()) {
                    continue;
                }
                // `latest_path` is unconditionally set to the scan-derived
                // path when the accumulator is brand new (no session-derived
                // clones), so a scan-only repo still has a sensible default
                // clone. An accumulator that already has a session-derived
                // `latest_path` keeps it — the session-derived recency wins.
                if entry.latest_path.is_empty() {
                    entry.latest_path = clone.path.clone();
                }
                entry.clones.push(RepoClone {
                    path: clone.path,
                    last_opened_at: None,
                    last_branch: None,
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
                // Dedup clones by path. After the sort above the most-recent entry comes
                // first within each path, so retaining the first occurrence keeps the
                // latest branch_at_launch and drops the older repo_root's row.
                let mut seen = std::collections::HashSet::new();
                acc.clones.retain(|c| seen.insert(c.path.clone()));
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
    /// The set of paths already present in `clones`, used to de-dup when a
    /// scan-derived clone path matches one already added from the
    /// session-derived side.
    clone_paths: HashSet<String>,
}
