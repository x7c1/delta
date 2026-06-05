//! An event the Interactor emits for the browser to render.

use serde::Serialize;

use delta_model::{MessageUuid, SessionId};

/// An event the Interactor emits for the browser to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    SessionRegistered { session_id: SessionId },
    /// A queued send was confirmed as a turn start.
    TurnStarted {
        session_id: SessionId,
        pending_send_id: i64,
        matched_uuid: MessageUuid,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput {
        session_id: SessionId,
        prompt: String,
    },
    /// A response completed.
    TurnCompleted {
        session_id: SessionId,
        stop_reason: Option<String>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: SessionId,
        request_id: i64,
        tool_name: String,
    },
}
