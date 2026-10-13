//! JSON shape of a session's lifecycle status.

use delta_model::SessionStatus;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a session's lifecycle status.
///
/// Mirrors the domain [`SessionStatus`] variant-for-variant; this wire twin
/// carries the serialization concerns the domain type must not know about:
/// the lowercase variant names and the TypeScript export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SessionStatus")]
pub enum WireSessionStatus {
    Spawning,
    Active,
    Ended,
    Failed,
}

impl From<SessionStatus> for WireSessionStatus {
    fn from(status: SessionStatus) -> Self {
        match status {
            SessionStatus::Spawning => WireSessionStatus::Spawning,
            SessionStatus::Active => WireSessionStatus::Active,
            SessionStatus::Ended => WireSessionStatus::Ended,
            SessionStatus::Failed => WireSessionStatus::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_serializes_lowercase() {
        for (status, expected) in [
            (SessionStatus::Spawning, "spawning"),
            (SessionStatus::Active, "active"),
            (SessionStatus::Ended, "ended"),
            (SessionStatus::Failed, "failed"),
        ] {
            assert_eq!(
                serde_json::to_value(WireSessionStatus::from(status)).unwrap(),
                serde_json::json!(expected),
            );
        }
    }
}
