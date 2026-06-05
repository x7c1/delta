//! `PreToolUse` payload.

use serde::Deserialize;

/// `PreToolUse` payload.
#[derive(Debug, Deserialize)]
pub struct PreToolUsePayload {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
}
