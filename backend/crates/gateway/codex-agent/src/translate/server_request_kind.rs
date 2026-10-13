//! The classification of a server-originated request.

use delta_usecase::AgentPermissionRequest;
use serde_json::Value;

#[cfg(doc)]
use delta_usecase::AgentEvent;

/// The classification of a server-originated request.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerRequestKind {
    /// A permission/approval request Delta models: it becomes a
    /// [`AgentEvent::PermissionRequested`] and is answered with a decision.
    Approval(AgentPermissionRequest),
    /// A server → client request Delta does not model. It must be surfaced as
    /// [`AgentEvent::UnsupportedInteraction`] and answered (with an error) so
    /// the session never silently hangs on it.
    Unsupported { method: String, detail_json: Value },
}
