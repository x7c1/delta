//! Tool permission requests.
//!
//! When an agent asks before running a tool, Delta records the request so the
//! browser can show it, answer it, and keep an audit trail. The decision goes
//! back the provider's way: for Claude, by waking the blocked permission hook;
//! for Codex, over the app-server connection. Claude also gets a row for every
//! tool call its `PreToolUse` hook reports, asked or not, which the matching
//! `tool_result` later resolves.

use crate::permission_status::PermissionStatus;
use crate::session_id::SessionId;

/// A recorded tool-permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub id: i64,
    pub session_id: SessionId,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input_json: String,
    /// The id of the tool call this request gates (Claude Code's `tool_use_id`,
    /// Codex's `itemId`). For Claude it correlates the request with the
    /// matching `tool_result` transcript line so the request can be resolved
    /// the moment the tool completes; a Codex request is resolved by its answer
    /// instead, so there the id is only recorded. `None` means the request was
    /// recorded without a correlating tool call id.
    pub tool_use_id: Option<String>,
    pub status: PermissionStatus,
    pub decision_reason: Option<String>,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// ISO-8601 timestamp once decided.
    pub decided_at: Option<String>,
}
