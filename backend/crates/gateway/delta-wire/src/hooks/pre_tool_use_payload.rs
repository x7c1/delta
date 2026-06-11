//! `PreToolUse` payload.

use serde::{Deserialize, Serialize};

/// `PreToolUse` payload.
#[derive(Debug, Deserialize, Serialize)]
pub struct PreToolUsePayload {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    /// The id of the imminent tool call, e.g. `"toolu_0166..."`. It is the exact
    /// key Claude Code later writes as `tool_use_id` on the matching
    /// `tool_result` transcript line, so Delta can correlate the recorded
    /// permission request with its completion and auto-clear the notice.
    pub tool_use_id: String,
}
