//! The origin/host guard applied to every route.
//!
//! These drive the assembled router with `tower`'s `oneshot`, so they assert
//! the guard's 403 at the app boundary — before any handler runs. For the
//! WebSocket routes that matters especially: a foreign `Origin` must be refused
//! at the upgrade, before the socket opens, and the guard being a router-wide
//! layer is what makes a plain GET carrying the foreign `Origin` enough to prove
//! it (no handshake headers needed — the layer intercepts first).

use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// Build a bare GET request to `uri`, then let the caller layer headers on.
fn get(uri: &str) -> axum::http::request::Builder {
    Request::builder().method("GET").uri(uri)
}

#[tokio::test]
async fn rejects_a_cross_site_origin_on_a_rest_route() {
    let response = router(test_state().await)
        .oneshot(
            get("/api/providers")
                .header("host", "127.0.0.1")
                .header("origin", "https://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rejects_a_cross_site_origin_on_the_websocket_upgrades() {
    // Every live channel — `/ws`, `/pty`, `/comms` — must refuse a foreign
    // origin at the upgrade. The guard runs before the handler, so a GET
    // carrying the foreign origin is refused whether or not it is a valid
    // upgrade handshake.
    for uri in [
        "/ws",
        "/pty?session_id=whatever",
        "/comms?session_id=whatever",
    ] {
        let response = router(test_state().await)
            .oneshot(
                get(uri)
                    .header("host", "127.0.0.1")
                    .header("origin", "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "a foreign origin must be refused on {uri}",
        );
    }
}

#[tokio::test]
async fn rejects_a_non_loopback_host() {
    let response = router(test_state().await)
        .oneshot(
            get("/api/providers")
                .header("host", "evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn allows_a_request_with_no_origin_header() {
    // The Claude Code hook `curl`s and same-origin non-browser clients send no
    // `Origin`; with a loopback `Host` they must pass the guard. `/health` is
    // the simplest route with a determinate 200 body.
    let response = router(test_state().await)
        .oneshot(
            get("/health")
                .header("host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn allows_a_loopback_origin() {
    let response = router(test_state().await)
        .oneshot(
            get("/health")
                .header("host", "127.0.0.1:7878")
                .header("origin", "http://localhost:5173")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
