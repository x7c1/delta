//! The opt-in request to start a session inside a git worktree.

use crate::ports::WorktreeStartPoint;

/// An opt-in request to start a fresh session inside a git worktree.
///
/// When a `NewSession` carries a `WorktreeSpec` and a selected working
/// directory that is a git repository, Delta creates a per-session worktree and
/// launches the session there instead of in the selected directory itself. The
/// only knob is where the worktree's branch starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpec {
    /// Where the new worktree's branch should start from (current `HEAD` or a
    /// fetched remote branch).
    pub start_point: WorktreeStartPoint,
}
