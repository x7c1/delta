//! `SessionEnd` payload.

use serde::Deserialize;

/// `SessionEnd` payload.
///
/// Claude Code reports the ending session's id and why it ended. The `reason`
/// is optional and carried for observability only.
#[derive(Debug, Deserialize)]
pub struct SessionEndPayload {
    pub session_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}
