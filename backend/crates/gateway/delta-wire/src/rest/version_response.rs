//! Response for `GET /api/version`.

use serde::Serialize;
use ts_rs::TS;

/// Response for `GET /api/version`: the version string the browser footer
/// displays.
///
/// `version` is a pre-formatted string that already carries the leading `v`
/// and, on debug builds, the `+dev.<short-sha>` SemVer build-metadata suffix
/// (see `delta_server::version::display_version` for the format contract).
/// The browser renders it verbatim — the server owns the format so a UI
/// change never needs to know how to parse the base version and sha apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "VersionResponse")]
pub struct WireVersionResponse {
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_response_serializes_with_the_rest_field_name() {
        assert_eq!(
            serde_json::to_value(WireVersionResponse {
                version: "v0.2.1".to_owned(),
            })
            .unwrap(),
            serde_json::json!({ "version": "v0.2.1" }),
        );
    }
}
