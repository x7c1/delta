//! The opt-in request to start a session inside a git worktree.

use crate::ports::WorktreeStartPoint;

/// An opt-in request to start a fresh session inside a git worktree.
///
/// When a `NewSession` carries a `WorktreeSpec` and a selected working
/// directory that is a git repository, Delta creates a per-session worktree and
/// launches the session there instead of in the selected directory itself.
/// `start_point` says where the worktree's branch starts from;
/// `confirmed_commit` carries the user's answer to Delta's launch
/// confirmation, when one was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpec {
    /// Where the new worktree's branch should start from (current `HEAD` or a
    /// fetched remote branch).
    pub start_point: WorktreeStartPoint,
    /// The commit the user confirmed in Delta's launch confirmation (the
    /// commit the launch-trust check reported), or `None` when no
    /// confirmation was given.
    ///
    /// The confirmation is bound to it: Claude Code's trust is pre-accepted on
    /// the strength of the confirmation only when the built worktree is on
    /// exactly this commit. A worktree on any other commit is not pre-accepted
    /// and the launch fails, so the user is asked again about what is actually
    /// there.
    pub confirmed_commit: Option<String>,
}

impl WorktreeSpec {
    /// A request with no launch confirmation attached.
    pub fn new(start_point: WorktreeStartPoint) -> Self {
        Self {
            start_point,
            confirmed_commit: None,
        }
    }
}
