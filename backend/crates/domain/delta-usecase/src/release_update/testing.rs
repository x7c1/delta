//! Test helpers shared by the release update's tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use semver::Version;
use tokio::sync::{mpsc, Mutex, Notify};

use super::{lock, Platform, ReleaseUpdate, UpdateDownload, RELEASE_DOWNLOAD_PREFIX};
use crate::ports::{
    AssetDownloadError, AssetDownloader, DownloadProgress, InstallError, InstalledApp,
    ReleaseAsset, UpdateInstaller,
};
use crate::release_check::RELEASE_PAGE_PREFIX;
use crate::NewerRelease;

/// The Linux asset of v0.6.0 the tests download.
pub(super) const LINUX_DEB: &str = "delta-desktop_0.6.0_amd64.deb";

/// The macOS asset of v0.6.0 the tests download.
pub(super) const MAC_DMG: &str = "Delta_0.6.0_aarch64.dmg";

/// The sha256 every test asset states, lowercase hex.
pub(super) fn test_sha256() -> String {
    "0".repeat(64)
}

/// The app the tests' installer names for every install it ends `Ok`.
pub(super) fn installed_app() -> InstalledApp {
    InstalledApp::Executable(PathBuf::from("/usr/bin/delta-desktop"))
}

/// An asset named `name` under the pinned download prefix, with a digest.
pub(super) fn asset(name: &str) -> ReleaseAsset {
    ReleaseAsset {
        name: name.to_owned(),
        download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v0.6.0/{name}"),
        digest: Some(format!("sha256:{}", test_sha256())),
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

/// An installer whose every install waits until the test hands it an
/// outcome with [`Self::finish`], recording what it was asked to install.
/// An install it ends `Ok` names [`installed_app`]. It may install here
/// unless made with [`Self::unavailable_because`].
pub(super) struct GatedInstaller {
    unavailable: Option<String>,
    asked: std::sync::Mutex<Vec<(String, PathBuf, String)>>,
    outcome_tx: mpsc::UnboundedSender<Result<(), InstallError>>,
    outcome_rx: Mutex<mpsc::UnboundedReceiver<Result<(), InstallError>>>,
}

impl GatedInstaller {
    pub(super) fn new() -> Arc<Self> {
        Self::with(None)
    }

    /// An installer that says up front it cannot install here, for `reason`
    /// ([`UpdateInstaller::unavailable`]).
    pub(super) fn unavailable_because(reason: &str) -> Arc<Self> {
        Self::with(Some(reason.to_owned()))
    }

    fn with(unavailable: Option<String>) -> Arc<Self> {
        let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            unavailable,
            asked: std::sync::Mutex::new(Vec::new()),
            outcome_tx,
            outcome_rx: Mutex::new(outcome_rx),
        })
    }

    /// The `(version, file, sha256)` of every install started so far.
    pub(super) fn asked(&self) -> Vec<(String, PathBuf, String)> {
        self.asked.lock().unwrap().clone()
    }

    /// End the running (or next) install with `outcome`.
    pub(super) fn finish(&self, outcome: Result<(), InstallError>) {
        self.outcome_tx.send(outcome).unwrap();
    }
}

#[async_trait]
impl UpdateInstaller for GatedInstaller {
    async fn install(
        &self,
        version: &str,
        file: &Path,
        sha256: &str,
    ) -> Result<InstalledApp, InstallError> {
        self.asked.lock().unwrap().push((
            version.to_owned(),
            file.to_path_buf(),
            sha256.to_owned(),
        ));
        self.outcome_rx
            .lock()
            .await
            .recv()
            .await
            .unwrap()
            .map(|()| installed_app())
    }

    fn unavailable(&self) -> Option<String> {
        self.unavailable.clone()
    }
}

/// An update offered on `platform`, into `/data/updates`, through
/// `downloader` and `installer`.
pub(super) fn update_on(
    platform: Platform,
    downloader: &Arc<GatedDownloader>,
    installer: &Arc<GatedInstaller>,
) -> ReleaseUpdate {
    ReleaseUpdate::offered(
        platform,
        PathBuf::from("/data/updates"),
        Arc::clone(downloader) as Arc<dyn AssetDownloader>,
        Arc::clone(installer) as Arc<dyn UpdateInstaller>,
    )
}

/// An update offered on Linux x86_64 through `downloader`.
pub(super) fn linux_update(downloader: &Arc<GatedDownloader>) -> ReleaseUpdate {
    update_on(
        Platform::new("linux", "x86_64"),
        downloader,
        &GatedInstaller::new(),
    )
}

/// The newer release v0.6.0, carrying [`LINUX_DEB`].
pub(super) fn v060() -> NewerRelease {
    newer_release("0.6.0", vec![asset(LINUX_DEB)])
}

/// Wait until the background transfer has recorded its outcome.
pub(super) async fn settled(update: &ReleaseUpdate) -> UpdateDownload {
    for _ in 0..200 {
        match lock(&update.download).clone() {
            Some(UpdateDownload::Downloading { .. }) | None => {}
            Some(done) => return done,
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the download never settled");
}
