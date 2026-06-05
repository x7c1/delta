//! Tool permission requests.
//!
//! When a `PreToolUse` hook fires, a permission prompt is imminent in the TUI.
//! Delta does not decide allow/deny — the TUI handles that — but it records the
//! request so the browser can show state and keep an audit trail.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ids::SessionId;

/// Disposition of a recorded permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionStatus {
    /// Awaiting the user's decision in the TUI.
    Pending,
    Allowed,
    Denied,
}

impl PermissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionStatus::Pending => "pending",
            PermissionStatus::Allowed => "allowed",
            PermissionStatus::Denied => "denied",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(PermissionStatus::Pending),
            "allowed" => Ok(PermissionStatus::Allowed),
            "denied" => Ok(PermissionStatus::Denied),
            other => Err(Error::InvalidVariant {
                kind: "PermissionStatus",
                value: other.to_owned(),
            }),
        }
    }
}

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
