//! Tool permission requests.
//!
//! When a `PreToolUse` hook fires, a permission prompt is imminent in the TUI.
//! Delta does not decide allow/deny — the TUI handles that — but it records the
//! request so the browser can show state and keep an audit trail.

use serde::{Deserialize, Serialize};

use crate::ids::SessionId;
use crate::permission_status::PermissionStatus;

/// A recorded tool-permission request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: i64,
    pub session_id: SessionId,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input_json: String,
    pub status: PermissionStatus,
    pub decision_reason: Option<String>,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// ISO-8601 timestamp once decided.
    pub decided_at: Option<String>,
}
