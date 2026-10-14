//! The state of a newer release's download.

use delta_usecase::UpdateDownload;
use serde::Serialize;
use ts_rs::TS;

/// The state of a newer release's download, internally tagged by `state`.
///
/// - `downloading`: the transfer is running; `received_bytes` so far, of
///   `total_bytes` when the answer stated it.
/// - `ready`: downloaded and verified against its sha256 and the release's
///   signature, waiting to be applied.
/// - `unavailable`: the release cannot be updated to in the app: it carries
///   no signature of this platform's asset, or the signature does not verify
///   the downloaded file. Nothing was kept; `error` says why. A new request
///   for the same release answers this again; the user updates by hand from
///   the release page.
/// - `failed`: nothing was kept; `error` says why. A new request starts over.
///
/// `version` is the release the download is of (`v0.6.0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(rename = "UpdateDownload")]
pub enum WireUpdateDownload {
    Downloading {
        version: String,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    Ready {
        version: String,
    },
    Unavailable {
        version: String,
        error: String,
    },
    Failed {
        version: String,
        error: String,
    },
}

impl From<UpdateDownload> for WireUpdateDownload {
    fn from(download: UpdateDownload) -> Self {
        match download {
            UpdateDownload::Downloading { version, progress } => Self::Downloading {
                version,
                received_bytes: progress.received,
                total_bytes: progress.total,
            },
            UpdateDownload::Ready { version, .. } => Self::Ready { version },
            UpdateDownload::Unavailable { version, cause } => Self::Unavailable {
                version,
                error: cause,
            },
            UpdateDownload::Failed { version, cause } => Self::Failed {
                version,
                error: cause,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_download_state_serializes_in_snake_case() {
        assert_eq!(
            serde_json::to_value(WireUpdateDownload::Ready {
                version: "v0.6.0".to_owned()
            })
            .unwrap(),
            serde_json::json!({ "state": "ready", "version": "v0.6.0" })
        );
        assert_eq!(
            serde_json::to_value(WireUpdateDownload::Unavailable {
                version: "v0.6.0".to_owned(),
                error: "release v0.6.0 carries no signature".to_owned(),
            })
            .unwrap(),
            serde_json::json!({
                "state": "unavailable",
                "version": "v0.6.0",
                "error": "release v0.6.0 carries no signature",
            })
        );
        assert_eq!(
            serde_json::to_value(WireUpdateDownload::Failed {
                version: "v0.6.0".to_owned(),
                error: "the download answered with HTTP status 404".to_owned(),
            })
            .unwrap(),
            serde_json::json!({
                "state": "failed",
                "version": "v0.6.0",
                "error": "the download answered with HTTP status 404",
            })
        );
    }
}
