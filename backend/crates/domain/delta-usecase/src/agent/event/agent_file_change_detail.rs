//! What a permission request would do to files on disk.

use super::AgentFileChange;

/// What a permission request would do to files on disk, when the provider says
/// so before asking.
///
/// This is *display* detail for a live prompt: it exists so the approval card
/// can name the files it is gating and show the diff, instead of a truncated
/// blob of request params. It is `Some` only when the provider actually stated
/// the change set — the adapter never synthesises an empty detail, so a `None`
/// means "nothing is known here" and the card falls back to its input summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFileChangeDetail {
    /// The files the request would change, in the order the provider listed
    /// them.
    pub changes: Vec<AgentFileChange>,
    /// The provider's own explanation for why it is asking (Codex's `reason`,
    /// e.g. a request for write access outside the sandbox). `None` when the
    /// provider offered none.
    pub reason: Option<String>,
}
