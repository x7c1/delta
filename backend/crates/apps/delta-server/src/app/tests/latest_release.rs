//! `GET /api/latest-release`: the release check's last verdict and what the
//! browser may offer; `POST /api/latest-release/download`: downloading it.

use std::path::{Path, PathBuf};

use super::*;
use async_trait::async_trait;
use axum::http::StatusCode;
use delta_bootstrap::{BuildOrigin, Launcher};
use delta_usecase::{
    AssetDownloadError, AssetDownloader, DownloadProgress, Platform, PublishedRelease,
    ReleaseAsset, ReleaseCheck, ReleaseFeed, ReleaseFeedError, ReleaseUpdate,
};
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

/// A desktop release build on Linux x86_64 that knows of [`NEWER_TAG`] and
/// downloads through `downloader`.
async fn desktop_release_state(downloader: &Arc<GatedDownloader>) -> AppState {
    AppState::build(&test_config())
        .await
        .unwrap()
        .with_release_check(newer_check().await)
        .with_release_update(ReleaseUpdate::offered(
            Platform::new("linux", "x86_64"),
            PathBuf::from("/data/updates"),
            Arc::clone(downloader) as Arc<dyn AssetDownloader>,
        ))
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
        serde_json::json!({ "newer": null, "offer": "none", "download": null })
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
