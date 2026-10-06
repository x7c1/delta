//! What erasing everything Delta left on the machine did.

use delta_model::SessionId;

use crate::session_prune::SessionKeptItem;
use crate::session_removal::{DiskItem, KeptItem};

/// The outcome of `Interactor::erase_everything`: what went, and what was
/// kept because it held the user's work (or could not be shown not to).
///
/// The data directory is not in here: the transport deletes it after the
/// server has stopped, which this use case knows nothing about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EraseReport {
    /// The sessions whose rows were removed, oldest first.
    pub removed_sessions: Vec<SessionId>,
    /// The worktrees, branches and trust entries removed, in the order they
    /// were removed: first those of the removed sessions, then the leftover
    /// worktrees under the worktree base. (A leftover's trust entry goes with
    /// it but is not listed: the Storage removal only logs a failure there.)
    pub removed: Vec<DiskItem>,
    /// What the removed sessions kept on disk, each with its session — the
    /// same items removing each session alone would have kept.
    pub kept: Vec<SessionKeptItem>,
    /// The leftover directories under the worktree base that were kept, which
    /// belong to no session.
    pub kept_leftovers: Vec<KeptItem>,
}

impl EraseReport {
    /// The paths of the removed worktrees, in removal order.
    pub fn removed_worktrees(&self) -> impl Iterator<Item = &str> {
        self.removed.iter().filter_map(|item| match item {
            DiskItem::Worktree(path) => Some(path.as_str()),
            _ => None,
        })
    }

    /// The names of the removed branches, in removal order.
    pub fn removed_branches(&self) -> impl Iterator<Item = &str> {
        self.removed.iter().filter_map(|item| match item {
            DiskItem::Branch(name) => Some(name.as_str()),
            _ => None,
        })
    }
}
