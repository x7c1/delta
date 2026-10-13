//! A live handle to an open agent session.

use crate::agent::AgentProvider;

/// A live handle to an open agent session.
///
/// Besides the provider identity it carries an opaque adapter-local `key` the
/// adapter uses to address the session's underlying resource (for Claude, the
/// tmux pane token). The core treats `key` as opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionHandle {
    pub provider: AgentProvider,
    pub provider_session_id: String,
    pub key: String,
}
