//! What a permission request would do to files on disk.

use super::AgentFileChange;

/// What a permission request would do to files on disk, when the provider says
/// so before asking.
///
/// This is *display* detail for a live prompt: it exists so the approval card
/// can name the files it is gating and show the diff beside the request's
/// arguments. It is `Some` only when the request actually states the change set
/// — a correlated Codex patch, or a Claude file tool whose input carries the
/// change — and never an empty detail, so a `None` means "nothing is known
/// here" and the card shows the arguments alone.
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
