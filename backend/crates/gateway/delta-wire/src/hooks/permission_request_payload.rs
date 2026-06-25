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
    /// The JSONL the hook is firing against. For a nested subagent's tool call
    /// this is the subagent's own transcript, not the parent session's. The
    /// interactor compares this against the session row's stored path so a
    /// permission dialog raised inside a nested subagent does not race a
    /// parent-attributed waiter onto the wrong row.
    pub transcript_path: String,
}
