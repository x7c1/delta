//! The browser's answer to a pending permission request.

use delta_usecase::PermissionDecision;
use serde::Deserialize;
use ts_rs::TS;

/// The browser's answer to a pending permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "PermissionDecision")]
pub enum WirePermissionDecision {
    /// Permit this one request.
    Allow,
    /// Permit this request and comparable ones for the rest of the provider's
    /// session. Only providers whose
    /// [`WireProviderCapabilities::has_allow_for_session`] is `true` accept it;
    /// any other answers `400 permission_decision_unsupported` and leaves the
    /// request pending.
    ///
    /// [`WireProviderCapabilities::has_allow_for_session`]:
    ///     crate::rest::WireProviderCapabilities::has_allow_for_session
    AllowForSession,
    /// Refuse this request.
    Deny,
}

impl From<WirePermissionDecision> for PermissionDecision {
    fn from(decision: WirePermissionDecision) -> Self {
        match decision {
            WirePermissionDecision::Allow => PermissionDecision::Allow,
            WirePermissionDecision::AllowForSession => PermissionDecision::AllowForSession,
            WirePermissionDecision::Deny => PermissionDecision::Deny,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each wire value maps onto its own neutral decision: the session-scoped
    /// allow must NOT collapse into a plain `Allow` on the way in, or the
    /// provider would never be told to widen the grant.
    #[test]
    fn every_decision_maps_to_its_own_neutral_variant() {
        assert_eq!(
            PermissionDecision::from(WirePermissionDecision::Allow),
            PermissionDecision::Allow
        );
        assert_eq!(
            PermissionDecision::from(WirePermissionDecision::AllowForSession),
            PermissionDecision::AllowForSession
        );
        assert_eq!(
            PermissionDecision::from(WirePermissionDecision::Deny),
            PermissionDecision::Deny
        );
    }
}
