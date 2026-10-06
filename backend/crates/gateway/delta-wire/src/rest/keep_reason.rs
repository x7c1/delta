//! Why removing a session kept a piece of disk state.

use delta_usecase::KeepReason;
use serde::Serialize;
use ts_rs::TS;

/// Why removing a session left a worktree, branch or trust entry in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "KeepReason")]
pub enum WireKeepReason {
    /// The worktree has modified or untracked files.
    Dirty,
    /// The branch is not merged.
    Unmerged,
    /// The branch is not one Delta created for the session.
    NotCreatedByDelta,
    /// The branch is checked out in the worktree that was kept.
    WorktreeKept,
    /// Another listed session works in the worktree.
    InUseByAnotherSession,
    /// The operation failed; `detail` carries the error.
    Failed,
}

impl From<&KeepReason> for WireKeepReason {
    fn from(reason: &KeepReason) -> Self {
        match reason {
            KeepReason::Dirty => Self::Dirty,
            KeepReason::Unmerged => Self::Unmerged,
            KeepReason::NotCreatedByDelta => Self::NotCreatedByDelta,
            KeepReason::WorktreeKept => Self::WorktreeKept,
            KeepReason::InUseByAnotherSession => Self::InUseByAnotherSession,
            KeepReason::Failed(_) => Self::Failed,
        }
    }
}
