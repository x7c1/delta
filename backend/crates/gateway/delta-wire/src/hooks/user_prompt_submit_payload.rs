//! `UserPromptSubmit` payload.

use serde::Deserialize;

/// `UserPromptSubmit` payload.
#[derive(Debug, Deserialize)]
pub struct UserPromptSubmitPayload {
    pub prompt: String,
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
}
