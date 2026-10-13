//! One file attached to a published release.

/// One file attached to a published release, as the feed reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// The file's name (e.g. `delta-desktop_0.6.0_amd64.deb`).
    pub name: String,
    /// Where the file is downloaded from (GitHub's `browser_download_url`).
    pub download_url: String,
    /// The file's digest as the feed states it, `sha256:<hex>` on GitHub;
    /// `None` when the feed states none.
    pub digest: Option<String>,
}

/// The prefix of the only digest a release asset is verified against.
const SHA256_PREFIX: &str = "sha256:";

impl ReleaseAsset {
    /// The lowercase hex sha256 the asset's digest states, or `None` when it
    /// states no `sha256:<64 hex digits>` digest to verify the file against.
    pub fn sha256(&self) -> Option<String> {
        self.digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix(SHA256_PREFIX))
            .filter(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .map(str::to_ascii_lowercase)
    }
}
