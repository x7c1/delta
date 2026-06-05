//! `Stop` payload.

use serde::Deserialize;

/// `Stop` payload.
#[derive(Debug, Deserialize)]
pub struct StopPayload {
    pub session_id: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}
