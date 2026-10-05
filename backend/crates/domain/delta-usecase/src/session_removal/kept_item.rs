//! A piece of disk state removing a session left in place, and why.

use std::fmt;

use super::{DiskItem, KeepReason};

/// A [`DiskItem`] that removing its session kept, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptItem {
    /// What was kept.
    pub item: DiskItem,
    /// Why it was kept.
    pub reason: KeepReason,
}

impl fmt::Display for KeptItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.item, self.reason)
    }
}
