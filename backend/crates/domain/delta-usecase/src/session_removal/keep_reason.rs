//! Why removing a session left a piece of disk state in place.

use std::fmt;

/// Why a [`DiskItem`] was kept when its session was removed.
///
/// [`DiskItem`]: crate::DiskItem
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepReason {
    /// The worktree has modified or untracked files, so git refused to remove
    /// it.
    Dirty,
    /// The branch is not merged into its upstream (or `HEAD`), so git refused
    /// to delete it.
    Unmerged,
    /// The branch is not one Delta created for the session (the session was
    /// started on an existing branch, a pull request's for example), so it is
    /// the user's, never Delta's to delete.
    NotCreatedByDelta,
    /// The branch is still checked out in the worktree that was kept.
    WorktreeKept,
    /// Another session Delta still lists works in the same directory.
    InUseByAnotherSession,
    /// The operation failed for a reason other than git's refusal (the
    /// repository is gone, `git` is missing, the config file is unreadable).
    /// Carries the error message.
    Failed(String),
}

impl fmt::Display for KeepReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dirty => f.write_str("it has modified or untracked files"),
            Self::Unmerged => f.write_str("it is not merged"),
            Self::NotCreatedByDelta => f.write_str("Delta did not create it"),
            Self::WorktreeKept => f.write_str("its worktree was kept"),
            Self::InUseByAnotherSession => f.write_str("another session works in it"),
            Self::Failed(error) => write!(f, "removing it failed: {error}"),
        }
    }
}
