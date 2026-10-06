//! What a bulk removal of old sessions did.

use delta_model::SessionId;

use super::{SessionKeptItem, SkippedSession};

/// The outcome of removing old sessions in bulk.
///
/// `removed` and `skipped` together are every session that matched; `kept`
/// lists what the removed sessions left on disk because it held work (see
/// [`crate::SessionRemoval`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// The sessions removed, in the order they were removed (oldest first).
    pub removed: Vec<SessionId>,
    /// The matching sessions left in place, and why.
    pub skipped: Vec<SkippedSession>,
    /// The worktrees, branches and trust entries the removed sessions kept.
    pub kept: Vec<SessionKeptItem>,
}
