//! The request log ([`crate::request_log`]) on the assembled router.
//!
//! Each test drives a request through the full middleware stack and reads back
//! what a thread-local subscriber with a given `EnvFilter` recorded, so the
//! lines are asserted as they would reach the log file: their target, level and
//! fields, and what they leave out.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracing::subscriber::{DefaultGuard, NoSubscriber};
use tracing::Dispatch;
use tracing_subscriber::{fmt, EnvFilter};

/// The directive that turns the request log on and nothing else.
const REQUEST_LOG_ON: &str = "info,delta_server::http=debug";

/// The buffer a [`capture`] subscriber writes into.
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    /// Everything logged so far, as plain text (no ANSI, no timestamps).
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    /// The request lines logged so far.
    fn request_lines(&self) -> Vec<String> {
        self.text()
            .lines()
            .filter(|line| line.contains(crate::request_log::TARGET))
            .map(str::to_owned)
            .collect()
    }
}

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install a thread-local subscriber filtered by `directives` until the
/// returned guard drops, and hand back the buffer it writes to.
///
/// A no-op dispatcher is kept alive for the whole test process first, so that
/// `tracing` never caches a callsite's interest from another test thread's
/// lack of a subscriber (the same reason the use-case crate's warning capture
/// gives). The `#[tokio::test]` current-thread runtime keeps the middleware on
/// this thread.
fn capture(directives: &str) -> (LogCapture, DefaultGuard) {
    static ANCHOR: OnceLock<Dispatch> = OnceLock::new();
    ANCHOR.get_or_init(|| Dispatch::new(NoSubscriber::default()));
    let capture = LogCapture::default();
    let subscriber = fmt()
        .with_writer(capture.clone())
        .with_env_filter(EnvFilter::new(directives))
        .without_time()
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (capture, guard)
}

/// A loopback GET to `uri`, with the bearer token when `authorized`.
async fn get(state: AppState, uri: &str, authorized: bool) -> StatusCode {
    let mut request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "127.0.0.1");
    if authorized {
        request = request.header("authorization", bearer());
    }
    router(state)
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

/// The single request line in `logs`.
fn only_line(logs: &LogCapture) -> String {
    let lines = logs.request_lines();
    assert_eq!(lines.len(), 1, "one line per request: {:?}", logs.text());
    lines.into_iter().next().unwrap()
}

#[tokio::test]
async fn a_request_is_logged_with_its_route_path_status_and_time() {
    let state = test_state().await;
    let (logs, _guard) = capture(REQUEST_LOG_ON);

    let status = get(state, "/api/sessions/ghost/threads", true).await;

    let line = only_line(&logs);
    assert!(line.contains("DEBUG"), "{line}");
    assert!(line.contains("method=GET"), "{line}");
    assert!(
        line.contains("route=\"/api/sessions/{id}/threads\""),
        "{line}"
    );
    assert!(
        line.contains("path=\"/api/sessions/ghost/threads\""),
        "{line}"
    );
    assert!(
        line.contains(&format!("status={}", status.as_u16())),
        "{line}"
    );
    assert!(line.contains("elapsed_ms="), "{line}");
}

#[tokio::test]
async fn a_request_the_auth_guard_refuses_is_logged_with_401() {
    let state = test_state().await;
    let (logs, _guard) = capture(REQUEST_LOG_ON);

    let status = get(state, "/api/providers", false).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let line = only_line(&logs);
    assert!(line.contains("route=\"/api/providers\""), "{line}");
    assert!(line.contains("status=401"), "{line}");
}

#[tokio::test]
async fn a_request_the_origin_guard_refuses_is_logged_with_403() {
    let state = test_state().await;
    let (logs, _guard) = capture(REQUEST_LOG_ON);

    let response = router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/providers")
                .header("host", "evil.example")
                .header("authorization", bearer())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(only_line(&logs).contains("status=403"));
}

#[tokio::test]
async fn no_part_of_the_query_is_logged() {
    let state = test_state().await;
    let (logs, _guard) = capture(REQUEST_LOG_ON);

    get(
        state,
        "/api/sessions/ghost/threads?token=secret-run-token&cwd=/home/someone",
        false,
    )
    .await;

    let line = only_line(&logs);
    assert!(
        line.contains("path=\"/api/sessions/ghost/threads\""),
        "{line}"
    );
    for leaked in ["?", "token", "secret-run-token", "cwd", "/home/someone"] {
        assert!(!line.contains(leaked), "{leaked:?} leaked into {line}");
    }
}

#[tokio::test]
async fn the_default_info_filter_logs_no_request() {
    let state = test_state().await;
    let (logs, _guard) = capture("info");

    get(state, "/api/providers", true).await;

    assert!(logs.request_lines().is_empty(), "{:?}", logs.text());
}

#[tokio::test]
async fn a_static_web_request_is_logged_without_a_route() {
    use crate::app::{api_router, static_web, with_request_log};
    use include_dir::{include_dir, Dir};
    static FIXTURE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/web-dist");

    let app = with_request_log(static_web::mount(
        api_router(test_state().await),
        &FIXTURE,
        TEST_AUTH_TOKEN,
    ));
    let (logs, _guard) = capture(REQUEST_LOG_ON);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/assets/app-abc123.js")
                .header("host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let line = only_line(&logs);
    assert!(line.contains("route=\"-\""), "{line}");
    assert!(line.contains("path=\"/assets/app-abc123.js\""), "{line}");
    assert!(line.contains("status=200"), "{line}");
}
