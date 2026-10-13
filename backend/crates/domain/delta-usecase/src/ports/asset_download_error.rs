//! Why an asset download, or a reverify, failed.

use std::path::PathBuf;

use super::FeedCause;

/// Why an asset was not downloaded, or a file an earlier download left
/// behind was not verified again.
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
    /// The file an earlier download left behind is a symlink, which is not
    /// followed, or not a regular file at all.
    #[error("{} is not a regular file", path.display())]
    NotRegularFile { path: PathBuf },
    /// The file an earlier download left behind could not be opened or
    /// read.
    #[error("could not read {}", path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
