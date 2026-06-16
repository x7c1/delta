//! Request body for `PATCH /api/launch-options/{id}`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `PATCH /api/launch-options/{id}`.
///
/// Updates a registered launch option's `default_enabled` flag in place, so the
/// option's id and `created_at` are preserved (a delete+recreate would churn
/// both). `name`, `value`, and `label` are immutable through this endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "UpdateLaunchOptionRequest")]
pub struct WireUpdateLaunchOptionRequest {
    /// Whether the option starts pre-checked in the session-start picker.
    pub default_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_the_default_enabled_flag() {
        let req: WireUpdateLaunchOptionRequest =
            serde_json::from_str(r#"{"default_enabled":true}"#).unwrap();
        assert!(req.default_enabled);

        let req: WireUpdateLaunchOptionRequest =
            serde_json::from_str(r#"{"default_enabled":false}"#).unwrap();
        assert!(!req.default_enabled);
    }
}
