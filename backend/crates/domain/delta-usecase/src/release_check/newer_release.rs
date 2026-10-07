use semver::Version;

use crate::ports::ReleaseAsset;

/// A published release newer than this build.
///
/// It keeps the release's assets, so a download fetches exactly the release
/// the browser was told about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerRelease {
    pub(crate) version: Version,
    pub(crate) url: String,
    pub(crate) assets: Vec<ReleaseAsset>,
}

impl NewerRelease {
    /// The release's version rendered like the server's own display version:
    /// `v<version>` (e.g. `v0.6.0`).
    pub fn display_version(&self) -> String {
        format!("v{}", self.version)
    }

    /// The release's page, always under
    /// [`RELEASE_PAGE_PREFIX`](super::RELEASE_PAGE_PREFIX).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The release's version, without the `v`.
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// The files attached to the release, as the feed listed them.
    pub fn assets(&self) -> &[ReleaseAsset] {
        &self.assets
    }
}
