//! Test helpers shared by the release update's tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use semver::Version;
use tokio::sync::{mpsc, Mutex, Notify};

use super::RELEASE_DOWNLOAD_PREFIX;
use crate::ports::{AssetDownloadError, AssetDownloader, DownloadProgress, ReleaseAsset};
use crate::release_check::RELEASE_PAGE_PREFIX;
use crate::NewerRelease;

/// An asset named `name` under the pinned download prefix, with a digest.
pub(super) fn asset(name: &str) -> ReleaseAsset {
    ReleaseAsset {
        name: name.to_owned(),
        download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v0.6.0/{name}"),
        digest: Some(format!("sha256:{}", "0".repeat(64))),
    }
}

/// A newer release of `version` carrying `assets`.
pub(super) fn newer_release(version: &str, assets: Vec<ReleaseAsset>) -> NewerRelease {
    NewerRelease {
        version: Version::parse(version).unwrap(),
        url: format!("{RELEASE_PAGE_PREFIX}tag/v{version}"),
        assets,
    }
}

/// A downloader whose every transfer reports [`Self::PROGRESS`], then waits
/// until the test hands it an outcome with [`Self::finish`].
pub(super) struct GatedDownloader {
    calls: AtomicUsize,
    started: Notify,
    outcome_tx: mpsc::UnboundedSender<Result<(), AssetDownloadError>>,
    outcome_rx: Mutex<mpsc::UnboundedReceiver<Result<(), AssetDownloadError>>>,
}

impl GatedDownloader {
    /// What every transfer reports before it waits.
    pub(super) const PROGRESS: DownloadProgress = DownloadProgress {
        received: 512,
        total: Some(1024),
    };

    pub(super) fn new() -> Arc<Self> {
        let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            started: Notify::new(),
            outcome_tx,
            outcome_rx: Mutex::new(outcome_rx),
        })
    }

    /// How many transfers have started.
    pub(super) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// End the running (or next) transfer with `outcome`.
    pub(super) fn finish(&self, outcome: Result<(), AssetDownloadError>) {
        self.outcome_tx.send(outcome).unwrap();
    }

    /// Wait until a transfer has reported its progress.
    pub(super) async fn wait_started(&self) {
        self.started.notified().await;
    }
}

#[async_trait]
impl AssetDownloader for GatedDownloader {
    async fn download(
        &self,
        asset: &ReleaseAsset,
        dir: &Path,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
    ) -> Result<PathBuf, AssetDownloadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        progress(Self::PROGRESS);
        self.started.notify_one();
        let outcome = self.outcome_rx.lock().await.recv().await.unwrap();
        outcome.map(|()| dir.join(&asset.name))
    }
}
