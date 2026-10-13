//! Test helpers shared by the release update's tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use semver::Version;
use tokio::sync::{mpsc, oneshot, Mutex, Notify};

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
///
/// What it finds left behind for [`AssetDownloader::reverify`] is set with
/// [`Self::leave_behind`]: nothing unless set.
pub(super) struct GatedDownloader {
    calls: AtomicUsize,
    left_behind: std::sync::Mutex<Option<Result<bool, AssetDownloadError>>>,
    reverified: std::sync::Mutex<Vec<String>>,
    reverify_gate: std::sync::Mutex<Option<oneshot::Receiver<()>>>,
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
            left_behind: std::sync::Mutex::new(None),
            reverified: std::sync::Mutex::new(Vec::new()),
            reverify_gate: std::sync::Mutex::new(None),
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

    /// Have the next [`AssetDownloader::reverify`] find a matching file
    /// (`Ok(true)`), none (`Ok(false)`), or one that fails (`Err`).
    pub(super) fn leave_behind(&self, outcome: Result<bool, AssetDownloadError>) {
        *self.left_behind.lock().unwrap() = Some(outcome);
    }

    /// Have the next [`AssetDownloader::reverify`] wait, once it has
    /// recorded the asset it was asked about, until the returned sender
    /// sends.
    pub(super) fn hold_reverify(&self) -> oneshot::Sender<()> {
        let (tx, rx) = oneshot::channel();
        *self.reverify_gate.lock().unwrap() = Some(rx);
        tx
    }

    /// The asset names [`AssetDownloader::reverify`] was asked about.
    pub(super) fn reverified(&self) -> Vec<String> {
        self.reverified.lock().unwrap().clone()
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

    async fn reverify(
        &self,
        asset: &ReleaseAsset,
        dir: &Path,
    ) -> Result<Option<PathBuf>, AssetDownloadError> {
        self.reverified.lock().unwrap().push(asset.name.clone());
        let gate = self.reverify_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.await.unwrap();
        }
        match self.left_behind.lock().unwrap().take() {
            Some(Ok(true)) => Ok(Some(dir.join(&asset.name))),
            Some(Ok(false)) | None => Ok(None),
            Some(Err(err)) => Err(err),
        }
    }
}

/// An installer whose every install waits until the test hands it an
/// outcome with [`Self::finish`], recording what it was asked to install.
/// An install it ends `Ok` names [`installed_app`]. It may install here
/// unless made with [`Self::unavailable_because`].
pub(super) struct GatedInstaller {
    unavailable: Option<Box<dyn Fn() -> InstallError + Send + Sync>>,
    asked: std::sync::Mutex<Vec<(String, PathBuf, String)>>,
    outcome_tx: mpsc::UnboundedSender<Result<(), InstallError>>,
    outcome_rx: Mutex<mpsc::UnboundedReceiver<Result<(), InstallError>>>,
}

impl GatedInstaller {
    pub(super) fn new() -> Arc<Self> {
        Self::with(None)
    }

    /// An installer that says up front it cannot install here, with the
    /// error `because` makes ([`UpdateInstaller::unavailable`]).
    pub(super) fn unavailable_because(
        because: impl Fn() -> InstallError + Send + Sync + 'static,
    ) -> Arc<Self> {
        Self::with(Some(Box::new(because)))
    }

    fn with(unavailable: Option<Box<dyn Fn() -> InstallError + Send + Sync>>) -> Arc<Self> {
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

    fn unavailable(&self) -> Option<InstallError> {
        self.unavailable.as_ref().map(|because| because())
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

/// An installer's own reason it cannot install, caused by an I/O error, as
/// an installer that cannot tell where the app runs from has.
#[derive(Debug, thiserror::Error)]
#[error("could not tell where Delta runs from")]
pub(super) struct Unlocated(#[source] pub(super) std::io::Error);

/// [`InstallError::Unavailable`] for [`Unlocated`], caused by "no such file
/// from the test".
pub(super) fn unlocated() -> InstallError {
    InstallError::unavailable(Unlocated(std::io::Error::other(
        "no such file from the test",
    )))
}

/// What the browser is shown for [`unlocated`]: each level once, in order.
pub(super) const UNLOCATED_CAUSE: &str = "Delta cannot install the update itself: \
     could not tell where Delta runs from: no such file from the test";
