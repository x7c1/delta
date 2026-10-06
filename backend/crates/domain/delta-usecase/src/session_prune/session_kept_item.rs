//! A piece of disk state a removed session left behind, with the session.

use delta_model::SessionId;

use crate::session_removal::KeptItem;

/// A [`KeptItem`] of one of the sessions a bulk removal removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKeptItem {
    /// The removed session the item belonged to.
    pub session_id: SessionId,
    /// What was kept, and why.
    pub kept: KeptItem,
}
