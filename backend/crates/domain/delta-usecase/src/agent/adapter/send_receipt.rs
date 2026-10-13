//! The outcome of an [`AgentAdapter::send`].

#[cfg(doc)]
use super::AgentAdapter;

/// The outcome of a [`AgentAdapter::send`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendReceipt {
    /// The provider's id for the accepted prompt, when it returns one.
    pub provider_message_id: Option<String>,
}
