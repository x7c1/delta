//! `UserPromptSubmit` payload.

use serde::{Deserialize, Serialize};

/// `UserPromptSubmit` payload.
#[derive(Debug, Deserialize, Serialize)]
pub struct UserPromptSubmitPayload {
    pub prompt: String,
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
}
