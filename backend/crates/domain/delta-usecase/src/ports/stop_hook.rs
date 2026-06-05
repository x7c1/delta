//! Payload of a `Stop` hook.

use delta_model::SessionId;

/// Payload of a `Stop` hook.
#[derive(Debug, Clone)]
pub struct StopHook {
    pub session_id: SessionId,
    pub stop_reason: Option<String>,
    pub last_assistant_message: Option<String>,
}
