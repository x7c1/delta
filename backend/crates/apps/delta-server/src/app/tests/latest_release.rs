//! `GET /api/latest-release`: the release check's last verdict and what the
//! browser may offer; `POST /api/latest-release/download`: downloading it;
//! `POST /api/latest-release/install` and `/restart`: installing it and
//! restarting into it.

use std::path::{Path, PathBuf};

use super::*;
use async_trait::async_trait;
use axum::http::StatusCode;
use delta_bootstrap::{BuildOrigin, Launcher};
use delta_usecase::{
    AssetDownloadError, AssetDownloader, DownloadProgress, InstallError, Platform,
    PublishedRelease, ReleaseAsset, ReleaseCheck, ReleaseFeed, ReleaseFeedError, ReleaseUpdate,
    UpdateInstaller,
};

use crate::serve::ServerStopped;
use tokio::sync::{mpsc, Mutex};

/// The newer release the tests' feeds announce, with Linux's asset.
const NEWER_TAG: &str = "v99.0.0";
const LINUX_ASSET: &str = "delta-desktop_99.0.0_amd64.deb";

/// A feed that always answers what `answer` builds and counts how often it
/// was asked.
struct FixedFeed {
    answer: fn() -> Result<PublishedRelease, ReleaseFeedError>,
    calls: AtomicUsize,
}

#[async_trait]
impl ReleaseFeed for FixedFeed {
    async fn latest_release(&self) -> Result<PublishedRelease, ReleaseFeedError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.answer)()
    }

    fn url(&self) -> &str {
        "https://feed.invalid/fixed"
    }
}

fn release(tag: &str) -> PublishedRelease {
    let bare = tag.trim_start_matches('v');
    let asset = |name: String| ReleaseAsset {
        download_url: format!("https://github.com/x7c1/delta/releases/download/{tag}/{name}"),
        name,
        digest: Some(format!("sha256:{}", "a".repeat(64))),
    };
    PublishedRelease {
        tag_name: tag.into(),
        html_url: format!("https://github.com/x7c1/delta/releases/tag/{tag}"),
        assets: vec![
            asset(format!("Delta_{bare}_aarch64.dmg")),
            asset(format!("delta-desktop_{bare}_amd64.deb")),
        ],
    }
}

fn check_answering(answer: fn() -> Result<PublishedRelease, ReleaseFeedError>) -> ReleaseCheck {
    let feed = Arc::new(FixedFeed {
        answer,
        calls: AtomicUsize::new(0),
    });
    ReleaseCheck::new(feed as Arc<dyn ReleaseFeed>, env!("CARGO_PKG_VERSION"))
}

/// A check that has found [`NEWER_TAG`].
async fn newer_check() -> ReleaseCheck {
    let check = check_answering(|| Ok(release(NEWER_TAG)));
    check.check().await.unwrap();
    check
}

/// A downloader whose every transfer waits until the test hands it an
/// outcome, and which counts the transfers it started.
struct GatedDownloader {
    calls: AtomicUsize,
    outcome_tx: mpsc::UnboundedSender<Result<(), AssetDownloadError>>,
    outcome_rx: Mutex<mpsc::UnboundedReceiver<Result<(), AssetDownloadError>>>,
}

impl GatedDownloader {
    fn new() -> Arc<Self> {
        let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            outcome_tx,
            outcome_rx: Mutex::new(outcome_rx),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn finish(&self, outcome: Result<(), AssetDownloadError>) {
        self.outcome_tx.send(outcome).unwrap();
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
        progress(DownloadProgress {
            received: 0,
            total: Some(1024),
        });
        let outcome = self.outcome_rx.lock().await.recv().await.unwrap();
        outcome.map(|()| dir.join(&asset.name))
    }
}

/// An installer whose every install waits until the test hands it an
/// outcome, and which counts the installs it started.
struct GatedInstaller {
    calls: AtomicUsize,
    outcome_tx: mpsc::UnboundedSender<Result<(), InstallError>>,
    outcome_rx: Mutex<mpsc::UnboundedReceiver<Result<(), InstallError>>>,
}

impl GatedInstaller {
    fn new() -> Arc<Self> {
        let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            outcome_tx,
            outcome_rx: Mutex::new(outcome_rx),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn finish(&self, outcome: Result<(), InstallError>) {
        self.outcome_tx.send(outcome).unwrap();
    }
}

#[async_trait]
impl UpdateInstaller for GatedInstaller {
    async fn install(&self, version: &str, file: &Path) -> Result<(), InstallError> {
        assert_eq!(version, NEWER_TAG);
        assert_eq!(file, Path::new("/data/updates").join(LINUX_ASSET));
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcome_rx.lock().await.recv().await.unwrap()
    }
}

/// A desktop release build on `platform` that knows of [`NEWER_TAG`],
/// downloads through `downloader` and installs through `installer`.
async fn desktop_release_state_on(
    platform: Platform,
    downloader: &Arc<GatedDownloader>,
    installer: &Arc<GatedInstaller>,
) -> AppState {
    AppState::build(&test_config())
        .await
        .unwrap()
        .with_release_check(newer_check().await)
        .with_release_update(ReleaseUpdate::offered(
            platform,
            PathBuf::from("/data/updates"),
            Arc::clone(downloader) as Arc<dyn AssetDownloader>,
            Arc::clone(installer) as Arc<dyn UpdateInstaller>,
        ))
}

/// A desktop release build on Linux x86_64 that knows of [`NEWER_TAG`] and
/// downloads through `downloader`.
async fn desktop_release_state(downloader: &Arc<GatedDownloader>) -> AppState {
    desktop_release_state_on(
        Platform::new("linux", "x86_64"),
        downloader,
        &GatedInstaller::new(),
    )
    .await
}

/// `GET /api/latest-release`'s `download`, once it is no longer
/// `downloading`.
async fn settled_download(state: &AppState) -> serde_json::Value {
    for _ in 0..400 {
        let (_, body) = request_json(state, "GET", "/api/latest-release", None).await;
        if body["download"]["state"] != "downloading" {
            return body["download"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the download never settled");
}

async fn post_download(state: &AppState) -> (StatusCode, serde_json::Value) {
    request_json(state, "POST", "/api/latest-release/download", None).await
}

#[tokio::test]
async fn an_empty_feed_url_asks_nothing_and_answers_null() {
    // What `DELTA_RELEASE_FEED_URL=""` configures, built the way the binary
    // builds its state.
    let release_feed_url = crate::config::config_from_vars(|name| {
        (name == "DELTA_RELEASE_FEED_URL").then(|| "".into())
    })
    .release_feed_url;
    assert_eq!(release_feed_url, None);
    let state = AppState::build(&Config {
        release_feed_url,
        ..test_config()
    })
    .await
    .unwrap();

    assert!(
        state.spawn_release_check().is_none(),
        "a turned-off check spawns nothing, so nothing is ever asked"
    );
    let (status, body) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({
            "newer": null,
            "offer": "none",
            "download": null,
            "installs": false,
            "install": null,
        })
    );
}

#[tokio::test]
async fn a_newer_release_is_answered_with_its_version_and_page() {
    let feed = Arc::new(FixedFeed {
        answer: || Ok(release(NEWER_TAG)),
        calls: AtomicUsize::new(0),
    });
    let check = ReleaseCheck::new(
        Arc::clone(&feed) as Arc<dyn ReleaseFeed>,
        env!("CARGO_PKG_VERSION"),
    );
    check.check().await.unwrap();
    let state = AppState::build(&test_config())
        .await
        .unwrap()
        .with_release_check(check);

    let (status, body) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({
            "newer": {
                "version": "v99.0.0",
                "url": "https://github.com/x7c1/delta/releases/tag/v99.0.0",
            },
            "offer": "none",
            "download": null,
            "installs": false,
            "install": null,
        })
    );
    assert_eq!(
        feed.calls.load(Ordering::SeqCst),
        1,
        "the request answers from memory"
    );
}

#[tokio::test]
async fn a_failed_check_answers_null_not_an_error() {
    let check = check_answering(|| Err(ReleaseFeedError::Status(403)));
    assert!(check.check().await.is_err());
    let state = AppState::build(&test_config())
        .await
        .unwrap()
        .with_release_check(check);

    let (status, body) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["newer"], serde_json::Value::Null);
}

/// A desktop release build's offer also depends on the platform it runs on
/// ([`a_release_this_app_would_not_download_is_offered_no_update`]), so it is
/// pinned to Linux in [`a_desktop_release_build_downloads_the_newer_release_once`]
/// rather than taken from the host here.
#[tokio::test]
async fn the_offer_follows_the_launcher_and_the_build_origin() {
    for (launcher, build_origin, offer) in [
        (Launcher::Cli, BuildOrigin::Release, "none"),
        (Launcher::Cli, BuildOrigin::Local, "none"),
        (Launcher::Desktop, BuildOrigin::Local, "rebuild"),
    ] {
        let state = AppState::build(&Config {
            launcher,
            build_origin,
            ..test_config()
        })
        .await
        .unwrap()
        .with_release_check(newer_check().await);
        let (status, body) = request_json(&state, "GET", "/api/latest-release", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["offer"], offer, "{launcher:?} {build_origin:?}");
        assert_eq!(body["newer"]["version"], NEWER_TAG);
    }
}

#[tokio::test]
async fn a_download_is_refused_for_the_cli_and_a_local_build() {
    for (launcher, build_origin, code) in [
        (Launcher::Cli, BuildOrigin::Release, "update_cli_launcher"),
        (Launcher::Cli, BuildOrigin::Local, "update_cli_launcher"),
        (Launcher::Desktop, BuildOrigin::Local, "update_local_build"),
    ] {
        let state = AppState::build(&Config {
            launcher,
            build_origin,
            ..test_config()
        })
        .await
        .unwrap()
        .with_release_check(newer_check().await);
        let (status, body) = post_download(&state).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{launcher:?} {build_origin:?}"
        );
        assert_eq!(body["code"], code);
    }
}

#[tokio::test]
async fn a_download_is_refused_without_a_newer_release() {
    let up_to_date = check_answering(|| Ok(release("v0.0.1")));
    up_to_date.check().await.unwrap();
    for check in [
        ReleaseCheck::disabled(),
        check_answering(|| Ok(release(NEWER_TAG))),
        up_to_date,
    ] {
        let downloader = GatedDownloader::new();
        let state = desktop_release_state(&downloader)
            .await
            .with_release_check(check);
        let (status, body) = post_download(&state).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "update_no_newer_release");
        assert_eq!(downloader.calls(), 0);
    }
}

#[tokio::test]
async fn a_release_this_app_would_not_download_is_offered_no_update() {
    let without_asset = || {
        Ok(PublishedRelease {
            assets: Vec::new(),
            ..release(NEWER_TAG)
        })
    };
    let without_digest = || {
        let mut answer = release(NEWER_TAG);
        for asset in &mut answer.assets {
            asset.digest = None;
        }
        Ok(answer)
    };
    for answer in [
        without_asset as fn() -> Result<PublishedRelease, ReleaseFeedError>,
        without_digest,
    ] {
        let check = check_answering(answer);
        check.check().await.unwrap();
        let downloader = GatedDownloader::new();
        let state = desktop_release_state(&downloader)
            .await
            .with_release_check(check);

        let (status, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(latest["newer"]["version"], NEWER_TAG);
        assert_eq!(latest["offer"], "none", "{latest}");

        // A request that does not go through the footer is still refused.
        let (status, body) = post_download(&state).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "update_unsupported");
        assert!(
            body["error"].as_str().unwrap().contains(LINUX_ASSET),
            "{body}"
        );
        assert_eq!(downloader.calls(), 0);
    }
}

#[tokio::test]
async fn a_desktop_release_build_downloads_the_newer_release_once() {
    let downloader = GatedDownloader::new();
    let state = desktop_release_state(&downloader).await;

    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["offer"], "update");
    assert_eq!(latest["download"], serde_json::Value::Null);

    let (status, body) = post_download(&state).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["state"], "downloading");
    assert_eq!(body["version"], NEWER_TAG);

    // A second request while the first runs starts no second transfer.
    let (status, body) = post_download(&state).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["state"], "downloading");

    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["offer"], "update");
    assert_eq!(latest["download"]["state"], "downloading");

    downloader.finish(Ok(()));
    assert_eq!(
        settled_download(&state).await,
        serde_json::json!({ "state": "ready", "version": NEWER_TAG })
    );
    assert_eq!(downloader.calls(), 1);

    // Ready for this release: answered without downloading again.
    let (status, body) = post_download(&state).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "state": "ready", "version": NEWER_TAG })
    );
    assert_eq!(downloader.calls(), 1);
}

#[tokio::test]
async fn a_request_after_a_failed_download_starts_over() {
    let downloader = GatedDownloader::new();
    let state = desktop_release_state(&downloader).await;

    let (status, _) = post_download(&state).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    downloader.finish(Err(AssetDownloadError::Status(404)));
    assert_eq!(
        settled_download(&state).await,
        serde_json::json!({
            "state": "failed",
            "version": NEWER_TAG,
            "error": "the download answered with HTTP status 404",
        })
    );

    let (status, body) = post_download(&state).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["state"], "downloading");
    downloader.finish(Ok(()));
    assert_eq!(settled_download(&state).await["state"], "ready");
    assert_eq!(downloader.calls(), 2);
}

async fn post(state: &AppState, path: &str) -> (StatusCode, serde_json::Value) {
    request_json(state, "POST", path, None).await
}

/// `GET /api/latest-release`'s `install`, once it is no longer `installing`.
async fn settled_install(state: &AppState) -> serde_json::Value {
    for _ in 0..400 {
        let (_, body) = request_json(state, "GET", "/api/latest-release", None).await;
        if body["install"]["state"] != "installing" {
            return body["install"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the install never settled");
}

/// A Linux desktop release build whose download of [`NEWER_TAG`] is ready,
/// installing through `installer`.
async fn ready_state(installer: &Arc<GatedInstaller>) -> AppState {
    let downloader = GatedDownloader::new();
    let state =
        desktop_release_state_on(Platform::new("linux", "x86_64"), &downloader, installer).await;
    post_download(&state).await;
    downloader.finish(Ok(()));
    assert_eq!(settled_download(&state).await["state"], "ready");
    state
}

/// What `sudo apt install` is given for the ready download.
fn manual_command() -> String {
    format!("sudo apt install /data/updates/{LINUX_ASSET}")
}

#[tokio::test]
async fn an_install_is_refused_for_the_cli_and_a_local_build() {
    for (launcher, build_origin, code) in [
        (Launcher::Cli, BuildOrigin::Release, "update_cli_launcher"),
        (Launcher::Desktop, BuildOrigin::Local, "update_local_build"),
    ] {
        let state = AppState::build(&Config {
            launcher,
            build_origin,
            ..test_config()
        })
        .await
        .unwrap()
        .with_release_check(newer_check().await);
        let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
        assert_eq!(latest["installs"], false);
        let (status, body) = post(&state, "/api/latest-release/install").await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{launcher:?} {build_origin:?}"
        );
        assert_eq!(body["code"], code);
    }
}

#[tokio::test]
async fn an_install_is_refused_where_the_app_does_not_install_updates() {
    let downloader = GatedDownloader::new();
    let installer = GatedInstaller::new();
    let state =
        desktop_release_state_on(Platform::new("macos", "aarch64"), &downloader, &installer).await;
    post_download(&state).await;
    downloader.finish(Ok(()));
    assert_eq!(settled_download(&state).await["state"], "ready");

    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["installs"], false);
    let (status, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "update_install_unsupported");
    assert_eq!(installer.calls(), 0);
}

#[tokio::test]
async fn an_install_is_refused_until_the_download_is_ready() {
    let downloader = GatedDownloader::new();
    let installer = GatedInstaller::new();
    let state =
        desktop_release_state_on(Platform::new("linux", "x86_64"), &downloader, &installer).await;
    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["installs"], true);

    // Not downloaded, then downloading, then failed.
    let (status, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "update_not_ready");
    post_download(&state).await;
    let (_, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(body["code"], "update_not_ready");
    downloader.finish(Err(AssetDownloadError::Status(404)));
    assert_eq!(settled_download(&state).await["state"], "failed");
    let (_, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(body["code"], "update_not_ready");
    assert_eq!(installer.calls(), 0);
}

#[tokio::test]
async fn a_ready_download_installs_once_and_ends_installed() {
    let installer = GatedInstaller::new();
    let state = ready_state(&installer).await;

    let (status, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body,
        serde_json::json!({ "state": "installing", "version": NEWER_TAG })
    );
    // A second request while the first runs starts nothing.
    let (status, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["state"], "installing");
    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["install"]["state"], "installing");

    installer.finish(Ok(()));
    assert_eq!(
        settled_install(&state).await,
        serde_json::json!({ "state": "installed", "version": NEWER_TAG })
    );
    let (status, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "installed");
    assert_eq!(installer.calls(), 1);
    // Installed (or found installed already by the helper), the download is
    // kept: only a rejected file is removed.
    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["download"]["state"], "ready");
}

#[tokio::test]
async fn a_dismissed_password_dialog_leaves_the_download_ready() {
    let installer = GatedInstaller::new();
    let state = ready_state(&installer).await;
    post(&state, "/api/latest-release/install").await;
    installer.finish(Err(InstallError::Dismissed));
    assert_eq!(settled_install(&state).await, serde_json::Value::Null);
    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["download"]["state"], "ready");
    assert_eq!(latest["install"], serde_json::Value::Null);
}

#[tokio::test]
async fn an_install_delta_cannot_run_offers_the_manual_command() {
    let installer = GatedInstaller::new();
    let state = ready_state(&installer).await;
    post(&state, "/api/latest-release/install").await;
    installer.finish(Err(InstallError::Unavailable(
        "not authorized, or no polkit authentication agent is running".into(),
    )));
    assert_eq!(
        settled_install(&state).await,
        serde_json::json!({
            "state": "unavailable",
            "version": NEWER_TAG,
            "error": "Delta cannot install the update itself: not authorized, or no polkit authentication agent is running",
            "manual_command": manual_command(),
        })
    );
}

#[tokio::test]
async fn a_failed_install_reports_the_helpers_reason_and_the_manual_command() {
    let reason = "apt-get could not install the update: E: Unmet dependencies";
    let installer = GatedInstaller::new();
    let state = ready_state(&installer).await;
    post(&state, "/api/latest-release/install").await;
    installer.finish(Err(InstallError::Failed(reason.into())));
    assert_eq!(
        settled_install(&state).await,
        serde_json::json!({
            "state": "failed",
            "version": NEWER_TAG,
            "error": reason,
            "manual_command": manual_command(),
        })
    );
}

#[tokio::test]
async fn a_rejected_file_offers_no_manual_command_and_must_be_downloaded_again() {
    let reason = "the update file's sha256 is 00, not release v99.0.0's ff";
    let installer = GatedInstaller::new();
    let state = ready_state(&installer).await;
    post(&state, "/api/latest-release/install").await;
    installer.finish(Err(InstallError::Rejected(reason.into())));
    assert_eq!(
        settled_install(&state).await,
        serde_json::json!({ "state": "rejected", "version": NEWER_TAG, "error": reason })
    );
    let (_, latest) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(latest["download"], serde_json::Value::Null);
    let (status, body) = post(&state, "/api/latest-release/install").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "update_not_ready");
    assert_eq!(installer.calls(), 1);
}

#[tokio::test]
async fn a_restart_is_refused_until_the_update_is_installed() {
    let installer = GatedInstaller::new();
    let state = ready_state(&installer).await;
    let (status, body) = post(&state, "/api/latest-release/restart").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "update_not_installed");

    post(&state, "/api/latest-release/install").await;
    let (_, body) = post(&state, "/api/latest-release/restart").await;
    assert_eq!(body["code"], "update_not_installed");
    assert_eq!(
        state.take_stop_reason(),
        None,
        "nothing asked the server to stop"
    );

    installer.finish(Ok(()));
    assert_eq!(settled_install(&state).await["state"], "installed");
    let (status, _) = post(&state, "/api/latest-release/restart").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        state.take_stop_reason(),
        Some(ServerStopped::Restart {
            version: NEWER_TAG.to_owned()
        })
    );
}
