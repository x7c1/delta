use std::path::PathBuf;

use crate::ports::DownloadProgress;

/// The state of the newer release's download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateDownload {
    /// The transfer is running.
    Downloading {
        /// The release being downloaded, `v<version>`.
        version: String,
        progress: DownloadProgress,
    },
    /// The file is downloaded and verified against its digest and the
    /// release's signature, waiting to be applied.
    Ready {
        /// The release downloaded, `v<version>`.
        version: String,
        /// The verified file.
        path: PathBuf,
        /// The sha256 the file was verified against, lowercase hex: the
        /// release's digest for it, which an installer that checks the file
        /// again checks it against.
        sha256: String,
    },
    /// The release cannot be updated to in the app: it carries no signature
    /// of this platform's asset, or the signature does not verify the
    /// downloaded file. Nothing was kept, and a new request for the same
    /// release answers this again rather than downloading it; the user
    /// updates by hand from the release page.
    Unavailable {
        /// The release that cannot be updated to, `v<version>`.
        version: String,
        /// Why, with the whole cause chain.
        cause: String,
    },
    /// The download failed and left nothing behind; a new request starts
    /// over.
    Failed {
        /// The release whose download failed, `v<version>`.
        version: String,
        /// Why, with the whole cause chain.
        cause: String,
    },
}

impl UpdateDownload {
    /// The release this download is of, `v<version>`.
    pub fn version(&self) -> &str {
        match self {
            Self::Downloading { version, .. }
            | Self::Ready { version, .. }
            | Self::Unavailable { version, .. }
            | Self::Failed { version, .. } => version,
        }
    }
}
