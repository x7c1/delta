//! The worktree an accepted session still has to build, as the accept phase
//! planned it.

use crate::send_target::WorktreeSpec;

/// The git worktree an accepted-but-not-yet-launched session still has to
/// build before its agent can start.
///
/// The accept phase only *plans* the worktree (it computes the path the build
/// will land on, which costs at most a `git worktree list`); the build itself —
/// a `git fetch` plus a full checkout on a large repository — runs on the
/// launch task. These are the inputs that build needs, carried across.
#[derive(Debug, Clone)]
pub struct PlannedWorktree {
    /// The repository the worktree is cut from (the user-selected workdir's
    /// root, already resolved by the accept phase's gate).
    pub repo_root: String,
    /// That repository's short identity (`org/repo`), which shapes the
    /// worktree directory name. `None` when no origin is configured.
    pub repository_display_name: Option<String>,
    /// What the user asked for: where the worktree's branch starts from.
    pub spec: WorktreeSpec,
}
