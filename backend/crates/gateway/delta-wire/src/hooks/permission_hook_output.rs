//! The `hookSpecificOutput` envelope of a `PermissionRequest` hook response.

use serde::{Deserialize, Serialize};

use crate::hooks::permission_decision_body::PermissionDecisionBody;

/// The `hookSpecificOutput` envelope Claude Code expects for
/// `PermissionRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    pub decision: PermissionDecisionBody,
}
