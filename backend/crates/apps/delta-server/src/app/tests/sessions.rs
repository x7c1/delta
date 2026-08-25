//! Session, send and stream routes: the router's smoke tests.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_version_returns_a_version_string_shaped_like_v_prefixed() {
    // Smoke test the endpoint shape: the response is `{ version: "v..." }`
    // where the string starts with `v` followed by the workspace version.
    // The debug/release suffix branch is compile-time (unit-tested in
    // `crate::version`); here we just pin the JSON envelope.
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .uri("/api/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let version = body["version"].as_str().expect("version is a string");
    assert!(
        version.starts_with(&format!("v{}", env!("CARGO_PKG_VERSION"))),
        "expected the response to start with v<CARGO_PKG_VERSION>, got {version}",
    );
}

#[tokio::test]
async fn list_sessions_rejects_a_malformed_cursor() {
    // A non-decodable cursor is a client error, surfaced as 400 rather than
    // silently ignored or treated as the first page.
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .uri("/api/sessions?cursor=not-a-valid-cursor%21")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// The comms-log route exists and requires a session to watch: a request
/// without `session_id` is rejected by the query extractor before any stream
/// is opened, so a client bug cannot leave a socket tailing nothing. (The
/// stream's own replay-then-tail behaviour is asserted over the real stack in
/// the Codex full-loop suite, and at the hub level in `crate::comms_log`.)
#[tokio::test]
async fn comms_requires_a_session_id() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .uri("/comms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn release_send_replies_conflict_with_the_stable_code_when_not_releasable() {
    // The route exists and the SendNotReleasable error surfaces as a 409
    // carrying the stable `send_not_releasable` code the frontend
    // branches on. With a fresh store no send exists, which is one of the
    // conflict cases (unknown / never-restored / already-released rows
    // all take the same guarded-UPDATE path, pinned at the store and
    // interactor levels).
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sends/9999/release")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.get("code").and_then(serde_json::Value::as_str),
        Some("send_not_releasable"),
    );
}
