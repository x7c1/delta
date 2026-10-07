//! Response for `GET /api/latest-release`.

use delta_usecase::NewerRelease;
use serde::Serialize;
use ts_rs::TS;

/// Response for `GET /api/latest-release`: a published release newer than the
/// running server, if the server knows of one.
///
/// `newer` is `null` whenever the browser has nothing to tell the user: the
/// server has not checked yet, the last check found it up to date, the check
/// is turned off, or every check so far failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "LatestReleaseResponse")]
pub struct WireLatestReleaseResponse {
    pub newer: Option<WireNewerRelease>,
}

/// A published release newer than the running server.
///
/// `version` is rendered like `GET /api/version` renders a release build's
/// (`v0.6.0`); `url` is the release's page on GitHub, always under
/// `https://github.com/x7c1/delta/releases/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "NewerRelease")]
pub struct WireNewerRelease {
    pub version: String,
    pub url: String,
}

impl From<NewerRelease> for WireNewerRelease {
    fn from(release: NewerRelease) -> Self {
        Self {
            version: release.display_version(),
            url: release.url().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_release_serializes_with_the_rest_field_names() {
        assert_eq!(
            serde_json::to_value(WireLatestReleaseResponse {
                newer: Some(WireNewerRelease {
                    version: "v0.6.0".to_owned(),
                    url: "https://github.com/x7c1/delta/releases/tag/v0.6.0".to_owned(),
                }),
            })
            .unwrap(),
            serde_json::json!({
                "newer": {
                    "version": "v0.6.0",
                    "url": "https://github.com/x7c1/delta/releases/tag/v0.6.0",
                }
            }),
        );
    }

    #[test]
    fn no_newer_release_serializes_as_null() {
        assert_eq!(
            serde_json::to_value(WireLatestReleaseResponse { newer: None }).unwrap(),
            serde_json::json!({ "newer": null }),
        );
    }
}
