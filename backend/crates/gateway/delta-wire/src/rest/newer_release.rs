//! A published release newer than the running server.

use delta_usecase::NewerRelease;
use serde::Serialize;
use ts_rs::TS;

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
