//! `PermissionRequest` response.

use serde::{Deserialize, Serialize};

/// `PermissionRequest` response.
///
/// Claude Code's HTTP `PermissionRequest` hook consumes a decision only from
/// this exact envelope:
///
/// ```json
/// { "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } }
/// ```
///
/// `behavior` is `"allow"` or `"deny"`. When the hook has no decision to
/// report (the browser never answered before the deadline) the handler
/// returns an empty `200` instead of this body, and the tool call continues
/// through the normal interactive permission flow — the TUI prompt appears
/// exactly as it would have without the hook.
///
/// `Deserialize` is derived alongside `Serialize` because the `fake-claude`
/// test binary parses this same envelope from the response it receives, so
/// both sides of the contract share one definition.
#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionRequestResponse {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: PermissionHookOutput,
}

impl PermissionRequestResponse {
    /// Build a response carrying the browser's decision.
    pub fn decided(allow: bool) -> Self {
        Self {
            hook_specific_output: PermissionHookOutput {
                hook_event_name: "PermissionRequest".to_owned(),
                decision: PermissionDecisionBody {
                    behavior: if allow { "allow" } else { "deny" }.to_owned(),
                },
            },
        }
    }
}

/// The `hookSpecificOutput` envelope Claude Code expects for
/// `PermissionRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    pub decision: PermissionDecisionBody,
}

/// The decision body: `behavior` is `"allow"` or `"deny"`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionDecisionBody {
    pub behavior: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_serializes_into_the_exact_envelope() {
        assert_eq!(
            serde_json::to_value(PermissionRequestResponse::decided(true)).unwrap(),
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow" },
                }
            }),
        );
        assert_eq!(
            serde_json::to_value(PermissionRequestResponse::decided(false)).unwrap(),
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "deny" },
                }
            }),
        );
    }
}
