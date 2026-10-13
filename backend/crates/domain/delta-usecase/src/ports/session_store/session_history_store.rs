//! Aggregations over the session history: recent working directories, the
//! `open cwd` allowlist, and the Repository tab's clone rows.

use async_trait::async_trait;

use super::recent_workdir::RecentWorkdir;
use super::repository_clone_row::RepositoryCloneRow;
use super::SessionStore;
use crate::error::Result;

/// Aggregations over the session history: the recent-workdir list, the
/// `open cwd` allowlist, and the Repository tab's clone rows.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait SessionHistoryStore: Send + Sync {
    /// The distinct working directories sessions have run in, most-recently-used
    /// first, capped at `limit`.
    ///
    /// Derived from the `session.cwd` column (Delta keeps no separate
    /// working-directory history): one row per distinct `cwd`, ordered by the
    /// most recent activity of any session that used it. The recency key is the
    /// same one the session list uses — `COALESCE(last_activity_at, created_at)`
    /// (the denormalized last-activity column, falling back to `created_at`) —
    /// maxed across the sessions sharing that `cwd`, so a directory's place
    /// reflects when it was last actually worked in. Each row carries that
    /// recency timestamp for display.
    async fn recent_workdirs(&self, limit: u32) -> Result<Vec<RecentWorkdir>>;

    /// Whether `path` is a known working directory of some session or message.
    ///
    /// Backs the `open cwd` allowlist: the REST endpoint only spawns an
    /// external tool against a path Delta has actually shown the browser, so
    /// a hand-crafted request cannot point the editor at an arbitrary path on
    /// disk. The check is a set membership over `session.cwd`,
    /// `session.requested_workdir`, and `message.cwd` — the same three columns
    /// the UI surfaces cwd values from.
    ///
    /// The path is compared verbatim; canonicalisation is the caller's job
    /// (the browser always sends the same string the server sent it, so no
    /// normalisation is needed at this layer).
    async fn cwd_exists(&self, path: &str) -> Result<bool>;

    /// One row per `(repo_root, clone_path)` pair in the session history, for
    /// the Repository tab.
    ///
    /// Drawn from `session` rows with a non-null `repo_root` (sessions outside
    /// any git repo do not contribute); the clone path is
    /// `COALESCE(requested_workdir, cwd)` so worktree-managed cwds do not leak
    /// in. Each row carries the most recent activity at that pair (the
    /// `MAX(COALESCE(last_activity_at, created_at))` of its sessions) and the
    /// `branch_at_launch` of the latest such session.
    ///
    /// The result set is bounded by three caps to keep the Repository tab from
    /// growing without limit as new worktree-on spawns are recorded:
    ///
    /// - `worktree_base`: absolute path of `$DELTA_WORKTREE_BASE`. A clone
    ///   path is classified as **generated** when it lies under
    ///   `worktree_base` (matched as a `<base>/<child>` prefix — i.e. the path
    ///   begins with `worktree_base + "/"`), and **user** otherwise.
    /// - `active_repo_limit`: only the top-N most-recently-active
    ///   `repo_root`s pass; the rest drop wholesale.
    /// - `user_clone_limit`: per `repo_root`, cap on user-picked path rows.
    /// - `generated_clone_limit`: per `repo_root`, cap on machine-generated
    ///   (under-`worktree_base`) path rows. Kept separate from the user cap so
    ///   a burst of disposable worktrees cannot squeeze out user-meaningful
    ///   clones (the main tree, manual sibling clones).
    ///
    /// Ordering: most-recent first by `last_opened_at`, then by `repo_root`
    /// ASC and `clone_path` ASC for determinism. The interactor de-dups by
    /// `clone_path` again after grouping (the same path can appear under
    /// different `repo_root`s for the same upstream).
    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>>;
}

#[async_trait]
impl SessionHistoryStore for Box<dyn SessionStore> {
    async fn recent_workdirs(&self, limit: u32) -> Result<Vec<RecentWorkdir>> {
        (**self).recent_workdirs(limit).await
    }

    async fn cwd_exists(&self, path: &str) -> Result<bool> {
        (**self).cwd_exists(path).await
    }

    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>> {
        (**self)
            .repository_clone_rows(
                worktree_base,
                active_repo_limit,
                user_clone_limit,
                generated_clone_limit,
            )
            .await
    }
}
