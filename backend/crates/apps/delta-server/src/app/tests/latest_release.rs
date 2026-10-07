//! `GET /api/latest-release`: the release check's last verdict.

use super::*;
use async_trait::async_trait;
use delta_usecase::{PublishedRelease, ReleaseCheck, ReleaseFeed, ReleaseFeedError};

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
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "newer": null }));
}

#[tokio::test]
async fn a_newer_release_is_answered_with_its_version_and_page() {
    let feed = Arc::new(FixedFeed {
        answer: || {
            Ok(PublishedRelease {
                tag_name: "v99.0.0".into(),
                html_url: "https://github.com/x7c1/delta/releases/tag/v99.0.0".into(),
            })
        },
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
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({
            "newer": {
                "version": "v99.0.0",
                "url": "https://github.com/x7c1/delta/releases/tag/v99.0.0",
            }
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
    let feed = Arc::new(FixedFeed {
        answer: || Err(ReleaseFeedError::Status(403)),
        calls: AtomicUsize::new(0),
    });
    let check = ReleaseCheck::new(feed as Arc<dyn ReleaseFeed>, env!("CARGO_PKG_VERSION"));
    assert!(check.check().await.is_err());
    let state = AppState::build(&test_config())
        .await
        .unwrap()
        .with_release_check(check);

    let (status, body) = request_json(&state, "GET", "/api/latest-release", None).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "newer": null }));
}
