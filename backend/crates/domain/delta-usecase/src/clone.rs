//! One local clone of a repository and its per-clone state.

use crate::ports::WorktreeStartPoint;

#[cfg(doc)]
use crate::Repository;

/// One local clone of a repository: its absolute path and the per-clone state
/// derived from the session history at that path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clone {
    /// Absolute path of the clone (the dir the user picked at spawn time).
    pub path: String,
    /// Timestamp of the most recent activity at this clone (`MAX` of the
    /// sessions' `last_activity_at`/`created_at`), ISO-8601 UTC. `None` only
    /// when every contributing session is itself activity-less.
    pub last_opened_at: Option<String>,
    /// Local branch checked out by the most recent session at this clone
    /// (its `branch_at_launch`). `None` for sessions that recorded none (not
    /// a git repo, detached HEAD, or pre-dating the snapshot column).
    pub last_branch: Option<String>,
    /// Launch-option ids selected for the most recent session at this clone.
    /// Phase B always returns `[]` — per-session launch-option persistence is
    /// deferred to a follow-up PR (see `Repository tab` plan).
    pub last_launch_option_ids: Vec<i64>,
    /// Whether the most recent session at this clone opted into a worktree.
    /// Phase B always returns `false` — per-session worktree-state persistence
    /// is deferred to a follow-up PR.
    pub last_worktree_enabled: bool,
    /// The most recent session's worktree start point, when one was chosen.
    /// Phase B always returns `None` — per-session worktree-state persistence
    /// is deferred to a follow-up PR.
    pub last_worktree_start_point: Option<WorktreeStartPoint>,
}
