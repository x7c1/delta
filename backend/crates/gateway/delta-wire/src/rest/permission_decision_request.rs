//! Request body for `POST /api/permissions/{id}/decision`.

use serde::Deserialize;
use ts_rs::TS;

use crate::rest::permission_decision::WirePermissionDecision;

/// Request body for `POST /api/permissions/{id}/decision`: resolve the
/// pending permission request the notice is showing. A `409` reply means the
/// request is no longer awaiting a browser decision (it was already decided,
/// or its hook wait timed out and the interactive TUI prompt owns it now); a
/// `400 permission_decision_unsupported` means the request is still pending but
/// this session's provider has no meaning for the decision value sent.
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
        // The session-scoped allow travels under the same `snake_case` rule as
        // its siblings — pinned here because the generated TypeScript union the
        // browser posts from is derived from exactly these names.
        let body: WirePermissionDecisionRequest =
            serde_json::from_str(r#"{ "decision": "allow_for_session" }"#).unwrap();
        assert_eq!(body.decision, WirePermissionDecision::AllowForSession);
    }
}
