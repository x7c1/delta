//! Payload of a `UserPromptSubmit` hook.

use delta_model::SessionId;

/// Payload of a `UserPromptSubmit` hook.
#[derive(Debug, Clone)]
pub struct UserPromptSubmitHook {
    pub prompt: String,
    pub session_id: SessionId,
    pub transcript_path: String,
    pub cwd: String,
}
