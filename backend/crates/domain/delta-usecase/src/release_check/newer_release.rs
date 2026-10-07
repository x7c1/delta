use semver::Version;

/// A published release newer than this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerRelease {
    pub(super) version: Version,
    pub(super) url: String,
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
}
