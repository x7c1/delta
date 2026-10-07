//! The response for `GET /api/latest-release`.

use delta_usecase::NewerRelease;
use serde::Serialize;
use ts_rs::TS;

use super::{WireUpdateDownload, WireUpdateInstall, WireUpdateOffer};

/// Response for `GET /api/latest-release`: a published release newer than the
/// running server, if the server knows of one, what the browser may offer
/// next to it, and how its download and install are going.
///
/// `newer` is `null` whenever the browser has nothing to tell the user: the
/// server has not checked yet, the last check found it up to date, the check
/// is turned off, or every check so far failed. `download` is `null` until a
/// download of `newer` is asked for. `installs` is whether this app installs
/// a ready download itself (`POST /api/latest-release/install`: a desktop
/// release build on Linux). `install` is `null` until an install of `newer`
/// is asked for, and again after the user dismissed the password dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "LatestReleaseResponse")]
pub struct WireLatestReleaseResponse {
    pub newer: Option<WireNewerRelease>,
    pub offer: WireUpdateOffer,
    pub download: Option<WireUpdateDownload>,
    pub installs: bool,
    pub install: Option<WireUpdateInstall>,
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
                offer: WireUpdateOffer::Update,
                download: Some(WireUpdateDownload::Downloading {
                    version: "v0.6.0".to_owned(),
                    received_bytes: 512,
                    total_bytes: Some(1024),
                }),
                installs: true,
                install: None,
            })
            .unwrap(),
            serde_json::json!({
                "newer": {
                    "version": "v0.6.0",
                    "url": "https://github.com/x7c1/delta/releases/tag/v0.6.0",
                },
                "offer": "update",
                "download": {
                    "state": "downloading",
                    "version": "v0.6.0",
                    "received_bytes": 512,
                    "total_bytes": 1024,
                },
                "installs": true,
                "install": null,
            }),
        );
    }

    #[test]
    fn no_newer_release_serializes_as_null() {
        assert_eq!(
            serde_json::to_value(WireLatestReleaseResponse {
                newer: None,
                offer: WireUpdateOffer::None,
                download: None,
                installs: false,
                install: None,
            })
            .unwrap(),
            serde_json::json!({
                "newer": null,
                "offer": "none",
                "download": null,
                "installs": false,
                "install": null,
            }),
        );
    }
}
