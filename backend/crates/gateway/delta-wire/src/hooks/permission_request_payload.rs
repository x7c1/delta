//! `PermissionRequest` payload.
use serde::{Deserialize, Serialize};

/// `PermissionRequest` payload. Claude Code fires this only when an interactive
/// permission dialog actually appears. Unlike `PreToolUse` it carries no
/// `tool_use_id`, so the server correlates by (session, tool_name, tool_input).
#[derive(Debug, Deserialize, Serialize)]
pub struct PermissionRequestPayload {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
}
