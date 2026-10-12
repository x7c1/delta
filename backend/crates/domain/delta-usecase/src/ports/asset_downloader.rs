//! Downloading one file of a published release.
//!
//! The [`AssetDownloader`] port fetches a [`ReleaseAsset`] into a directory
//! and verifies it against the digest the feed stated for it. Which asset to
//! fetch, and whether its URL is one Delta trusts, is the release update's
//! job ([`crate::ReleaseUpdate`]); the downloader only fetches what it is
//! handed, and refuses to fetch anything it cannot verify.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::{FeedCause, ReleaseAsset};

/// How far a running download has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Bytes received so far.
    pub received: u64,
    /// The whole file's size, when the answer stated it.
    pub total: Option<u64>,
}

/// Why an asset was not downloaded.
///
/// Whatever the failure, no file is left behind: not under the asset's name,
/// and not under the temporary name it was written to.
#[derive(Debug, thiserror::Error)]
pub enum AssetDownloadError {
    /// The asset states no `sha256:<hex>` digest, so it could not be verified
    /// and is not downloaded at all.
    #[error("the asset {0:?} carries no sha256 digest, and is never downloaded unverified")]
    NoDigest(String),
    /// The asset's name is not a plain file name, so it has no place in the
    /// download directory.
    #[error("the asset name {0:?} is not a plain file name")]
    BadName(String),
    /// No answer arrived, or the transfer broke off: offline, DNS, TLS, a
    /// refused connection, a stalled or interrupted transfer.
    #[error("the download failed")]
    Request(#[source] FeedCause),
    /// The server answered with a non-2xx status.
    #[error("the download answered with HTTP status {0}")]
    Status(u16),
    /// The transfer ended before the size the answer stated.
    #[error("the transfer ended after {received} of {expected} bytes")]
    Truncated { received: u64, expected: u64 },
    /// The file's sha256 is not the one the release states.
    #[error("the downloaded file's sha256 is {actual}, not the release's {expected}")]
    DigestMismatch { expected: String, actual: String },
    /// The file could not be written, renamed or cleaned up around.
    #[error("could not write {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Fetches release assets.
#[async_trait]
pub trait AssetDownloader: Send + Sync {
    /// Download `asset` into `dir` (created when missing) and verify it
    /// against its digest, reporting progress as bytes arrive.
    ///
    /// The file reaches its final name, `dir/<asset name>`, only once its
    /// digest matches, so a file under that name is always a verified one;
    /// every other file in `dir` is then removed. On any failure nothing is
    /// left behind. Returns the final path.
    async fn download(
        &self,
        asset: &ReleaseAsset,
        dir: &Path,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<PathBuf, AssetDownloadError>;
}
