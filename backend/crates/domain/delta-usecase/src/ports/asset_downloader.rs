//! Downloading one file of a published release.
//!
//! The [`AssetDownloader`] port fetches a [`ReleaseAsset`] into a directory
//! and verifies it against the digest the feed stated for it. Which asset to
//! fetch, and whether its URL is one Delta trusts, is the release update's
//! job ([`crate::ReleaseUpdate`]); the downloader only fetches what it is
//! handed, and refuses to fetch anything it cannot verify. It also verifies
//! again a file an earlier download left behind
//! ([`AssetDownloader::reverify`]), so a restarted app need not fetch it again.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::{AssetDownloadError, DownloadProgress, ReleaseAsset};

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

    /// Verify again the file an earlier [`Self::download`] of `asset` left
    /// in `dir`, `dir/<asset name>`, against the asset's digest, fetching
    /// nothing.
    ///
    /// `dir` is the user's to write to, so the file is not trusted on the
    /// strength of the check it passed when it was downloaded: it is read
    /// without following a symlink, refused unless it is a regular file, and
    /// its sha256 computed again. Returns its path when it matches, and
    /// `None` when there is no such file (or no `dir`). A file that fails is
    /// removed. Every other file in `dir` — another release's, a partial
    /// download — is removed either way, as after a download.
    async fn reverify(
        &self,
        asset: &ReleaseAsset,
        dir: &Path,
    ) -> Result<Option<PathBuf>, AssetDownloadError>;
}
