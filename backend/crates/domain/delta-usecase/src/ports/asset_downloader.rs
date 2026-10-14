//! Downloading one file of a published release.
//!
//! The [`AssetDownloader`] port fetches a [`SignedAsset`] into a directory
//! and verifies it against the digest the feed stated for it and against the
//! release's signature of it. Which asset to fetch, and whether its URL (and
//! its signature's) is one Delta trusts, is the release update's job
//! ([`crate::ReleaseUpdate`]); the downloader only fetches what it is
//! handed, and refuses to fetch anything it cannot verify. It also verifies
//! again a file an earlier download left behind
//! ([`AssetDownloader::reverify`]), so a restarted app need not fetch it again.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::{AssetDownloadError, DownloadProgress, SignedAsset};

/// Fetches release assets.
#[async_trait]
pub trait AssetDownloader: Send + Sync {
    /// Download `signed`'s asset into `dir` (created when missing) and verify
    /// it against its digest and against its signature, fetched from
    /// `signed`'s signature URL, reporting progress as the asset's bytes
    /// arrive.
    ///
    /// The signature must be made with the release key and name the asset's
    /// file name and `signed`'s version; anything else fails as
    /// [`AssetDownloadError::SignatureRejected`]. The file reaches its final
    /// name, `dir/<asset name>`, only once both checks pass, and the
    /// signature is kept beside it as `dir/<asset name>.minisig`, so a file
    /// under that name is always a verified one; every other file in `dir`
    /// is then removed. On any failure nothing is left behind. Returns the
    /// final path.
    async fn download(
        &self,
        signed: &SignedAsset,
        dir: &Path,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<PathBuf, AssetDownloadError>;

    /// Verify again the file an earlier [`Self::download`] of `signed` left
    /// in `dir`, `dir/<asset name>`, against the asset's digest and the
    /// signature kept beside it, fetching nothing.
    ///
    /// `dir` is the user's to write to, so the file is not trusted on the
    /// strength of the check it passed when it was downloaded: it and its
    /// signature are read without following a symlink, refused unless they
    /// are regular files, and both checks run again. Returns its path when
    /// they pass, and `None` when there is no such file (or no `dir`). A
    /// file that fails, or that has no signature beside it, is removed.
    /// Every other file in `dir` — another release's, a partial download —
    /// is removed either way, as after a download.
    async fn reverify(
        &self,
        signed: &SignedAsset,
        dir: &Path,
    ) -> Result<Option<PathBuf>, AssetDownloadError>;
}
