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

    /// Everything kept, each with why: first what the removed sessions kept,
    /// then the kept leftovers.
    pub fn kept_items(&self) -> impl Iterator<Item = &KeptItem> {
        self.kept
            .iter()
            .map(|kept| &kept.kept)
            .chain(&self.kept_leftovers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::session_removal::KeepReason;

    #[test]
    fn kept_items_lists_the_sessions_items_then_the_leftovers() {
        let worktree = KeptItem {
            item: DiskItem::Worktree("/w/a".into()),
            reason: KeepReason::Dirty,
        };
        let branch = KeptItem {
            item: DiskItem::Branch("feature".into()),
            reason: KeepReason::Unmerged,
        };
        let leftover = KeptItem {
            item: DiskItem::Worktree("/w/b".into()),
            reason: KeepReason::NotRegistered,
        };
        let report = EraseReport {
            kept: [&worktree, &branch]
                .into_iter()
                .map(|kept| SessionKeptItem {
                    session_id: SessionId::from("sess-1"),
                    kept: kept.clone(),
                })
                .collect(),
            kept_leftovers: vec![leftover.clone()],
            ..EraseReport::default()
        };

        let kept: Vec<_> = report.kept_items().collect();

        assert_eq!(kept, [&worktree, &branch, &leftover]);
    }
}
