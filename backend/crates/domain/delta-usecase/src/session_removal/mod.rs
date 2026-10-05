//! [`SessionRemoval`] and the items it lists. The rule itself is on
//! `SessionContext::delete_session`.

mod disk_item;
pub use disk_item::DiskItem;
mod keep_reason;
pub use keep_reason::KeepReason;
mod kept_item;
pub use kept_item::KeptItem;

use std::fmt;

/// What removing a session did to the disk state Delta created for it.
///
/// Both lists are empty for a session that did not run in a worktree Delta
/// created (it ran in the user's own directory, or in a scratch directory):
/// there was nothing of Delta's on disk to consider. The session's rows are
/// deleted whatever this says — a kept item never refuses a removal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionRemoval {
    /// What was removed, in the order it was removed.
    pub removed: Vec<DiskItem>,
    /// What was kept, and why.
    pub kept: Vec<KeptItem>,
}

impl SessionRemoval {
    pub(crate) fn removed(&mut self, item: DiskItem) {
        self.removed.push(item);
    }

    pub(crate) fn kept(&mut self, item: DiskItem, reason: KeepReason) {
        self.kept.push(KeptItem { item, reason });
    }
}

impl fmt::Display for SessionRemoval {
    /// `removed: <items>; kept: <items with reasons>`, with `nothing` for an
    /// empty list — one line for the server log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("removed: ")?;
        write_list(f, &self.removed)?;
        f.write_str("; kept: ")?;
        write_list(f, &self.kept)
    }
}

fn write_list<T: fmt::Display>(f: &mut fmt::Formatter<'_>, items: &[T]) -> fmt::Result {
    if items.is_empty() {
        return f.write_str("nothing");
    }
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}
