//! One file a permission request would change, in provider-neutral form.

use super::AgentFileChangeKind;

/// One file a permission request would change, in provider-neutral form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFileChange {
    /// The path that would change, exactly as the provider named it.
    pub path: String,
    /// How it would change, or `None` when the provider named a kind this build
    /// does not model — the path and the diff are still worth showing, so an
    /// unknown kind loses the label rather than the whole entry.
    pub kind: Option<AgentFileChangeKind>,
    /// The unified diff of the proposed change, as the provider produced it.
    pub diff: String,
}
