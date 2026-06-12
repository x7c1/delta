//! Request body for `POST /api/permissions/{id}/decision`.

use delta_usecase::PermissionDecision;
use serde::Deserialize;
use ts_rs::TS;

/// The browser's answer to a pending permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "PermissionDecision")]
pub enum WirePermissionDecision {
    Allow,
    Deny,
}

impl From<WirePermissionDecision> for PermissionDecision {
    fn from(decision: WirePermissionDecision) -> Self {
        match decision {
            WirePermissionDecision::Allow => PermissionDecision::Allow,
            WirePermissionDecision::Deny => PermissionDecision::Deny,
        }
    }
}

/// Request body for `POST /api/permissions/{id}/decision`: resolve the
/// pending permission request the notice is showing. A `409` reply means the
/// request is no longer awaiting a browser decision (it was already decided,
/// or its hook wait timed out and the interactive TUI prompt owns it now).
#[derive(Debug, Deserialize, TS)]
#[ts(rename = "PermissionDecisionRequest")]
pub struct WirePermissionDecisionRequest {
    pub decision: WirePermissionDecision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_deserializes_from_snake_case() {
        let body: WirePermissionDecisionRequest =
            serde_json::from_str(r#"{ "decision": "allow" }"#).unwrap();
        assert_eq!(body.decision, WirePermissionDecision::Allow);
        let body: WirePermissionDecisionRequest =
            serde_json::from_str(r#"{ "decision": "deny" }"#).unwrap();
        assert_eq!(body.decision, WirePermissionDecision::Deny);
    }
}
