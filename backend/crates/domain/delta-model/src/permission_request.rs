//! Tool permission requests.
//!
//! When a `PreToolUse` hook fires, a permission prompt is imminent in the TUI.
//! Delta does not decide allow/deny — the TUI handles that — but it records the
//! request so the browser can show state and keep an audit trail.

use crate::permission_status::PermissionStatus;
use crate::session::SessionId;

/// A recorded tool-permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub id: i64,
    pub session_id: SessionId,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input_json: String,
    /// The id of the tool call this request gates (Claude Code's `tool_use_id`).
    /// It correlates the request with the matching `tool_result` transcript line
    /// so the request can be resolved the moment the tool completes. `None`
    /// means the request was recorded without a correlating tool call id.
    pub tool_use_id: Option<String>,
    pub status: PermissionStatus,
    pub decision_reason: Option<String>,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// ISO-8601 timestamp once decided.
    pub decided_at: Option<String>,
}
