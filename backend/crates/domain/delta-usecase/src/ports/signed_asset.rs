//! A release asset together with the signature it is checked against.

use semver::Version;

use super::ReleaseAsset;

/// A release asset to download, with what it is checked against besides its
/// digest: the release's signature of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedAsset {
    /// The asset itself, stating its sha256 digest.
    pub asset: ReleaseAsset,
    /// Where the asset's signature file, `<asset name>.minisig`, is
    /// downloaded from: the release's own asset of that name, under the same
    /// pinned prefix as the asset
    /// ([`RELEASE_DOWNLOAD_PREFIX`](crate::RELEASE_DOWNLOAD_PREFIX)).
    pub signature_url: String,
    /// The release's version, which the signature must name along with the
    /// asset's file name.
    pub version: Version,
}
