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
    /// this is the subagent's own transcript (under the parent's
    /// `<session>/subagents/`), not the parent session's. The interactor
    /// recognises that location so a permission dialog raised inside a nested
    /// subagent does not race a parent-attributed waiter onto the wrong row,
    /// and follows the session's own transcript when this names it at a new
    /// path (Claude Code moves it when the session enters a worktree).
    pub transcript_path: String,
}
