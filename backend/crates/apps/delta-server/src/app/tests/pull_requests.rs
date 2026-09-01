//! Pull-request listing routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn prs_returns_empty_with_gh_unavailable() {
    // With the gh stub answering "unavailable", the route must
    // return 200 + `{gh_available: false, pull_requests: []}` —
    // the PR tab degrades gracefully on a host with no gh.
    let response = router(test_state_with_unavailable_gh().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prs?lens=reviewer")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["gh_available"], false);
    assert_eq!(body["pull_requests"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn prs_accepts_the_author_lens_too() {
    // Same fallback path, exercised through the author lens, so a
    // typo in the per-lens dispatch fails this test loudly.
    let response = router(test_state_with_unavailable_gh().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prs?lens=author")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["gh_available"], false);
}

#[tokio::test]
async fn prs_rejects_an_unknown_lens_with_400() {
    // The router test does not script `gh`, so we cannot make the
    // happy path deterministic here without coupling to the host's
    // installed gh. Lens validation, however, fails before the use
    // case runs and is a pure router check — assert that.
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prs?lens=everyone")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn prs_rejects_a_missing_lens_with_400() {
    // axum's query extractor rejects a missing required field with
    // 400, so the handler does not have to special-case it.
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
