//! The per-run bearer-token guard applied to every route.
//!
//! Like the `origin_guard` group, these drive the assembled router with
//! `tower`'s `oneshot`, so they assert the guard's `401` at the app boundary —
//! before any handler runs, and before the socket opens for the WebSocket
//! upgrades. Every request carries a loopback `Host` so it is the token, not the
//! origin/host guard, that decides the outcome. `bearer()` and `TEST_AUTH_TOKEN`
//! come from the parent test module via `use super::*`.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// A bare GET request to `uri` with a loopback `Host`, ready for the caller to
/// layer a token (or not) on top.
fn get(uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "127.0.0.1")
}

#[tokio::test]
async fn rejects_a_request_without_a_token() {
    // A loopback caller with no `Authorization` (a local `curl`, say) clears the
    // origin/host guard but not the bearer guard: it must be refused with 401.
    let response = router(test_state().await)
        .oneshot(get("/api/providers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_a_request_with_a_wrong_token() {
    let response = router(test_state().await)
        .oneshot(
            get("/api/providers")
                .header("authorization", "Bearer not-the-real-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn accepts_a_request_with_the_valid_token() {
    // The complement of the two rejections: a valid bearer token reaches the
    // handler. `/api/providers` is a determinate GET with a 200 body.
    let response = router(test_state().await)
        .oneshot(
            get("/api/providers")
                .header("authorization", bearer())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn rejects_a_websocket_upgrade_without_a_token() {
    // The three live channels take the token from a `token=` query parameter,
    // because a browser cannot set headers on a WebSocket upgrade. Without a
    // valid one the upgrade is refused with 401 before the socket opens — the
    // guard is a router-wide layer, so a plain GET carrying no token proves it.
    for uri in [
        "/ws",
        "/pty?session_id=whatever",
        "/comms?session_id=whatever",
    ] {
        let response = router(test_state().await)
            .oneshot(get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a missing token must be refused on {uri}",
        );
    }
}

#[tokio::test]
async fn accepts_a_websocket_upgrade_with_a_token_query_param() {
    // A valid `token=` query param clears the guard. A plain GET (no upgrade
    // handshake) is refused past the guard with 400/426, never 401 — the point
    // is only that the token itself was accepted, so assert it is not 401.
    for uri in [
        format!("/ws?token={TEST_AUTH_TOKEN}"),
        format!("/pty?session_id=whatever&token={TEST_AUTH_TOKEN}"),
        format!("/comms?session_id=whatever&token={TEST_AUTH_TOKEN}"),
    ] {
        let response = router(test_state().await)
            .oneshot(get(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a valid token query param must clear the guard on {uri}",
        );
    }
}

#[tokio::test]
async fn exempts_hooks_and_health_from_the_bearer_token() {
    // `/health` (a liveness probe) is exempt from both guards, and `/hooks/*`
    // (the Claude Code control plane, `curl`ed with no place to carry a bearer
    // token) is exempt from *this* bearer guard — it authenticates through its
    // own `hs` secret instead (see the `hooks` group's `rejects_a_hook_without_
    // the_secret`). So each must be reachable with no bearer token — never 401.
    // `/health` answers 200; the hook endpoint here presents a valid `hs` and a
    // trivial body, so the guards let it through to the handler.
    let health = router(test_state().await)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .header("host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK, "/health needs no token");

    let hook = router(test_state().await)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/hooks/status-line{}", hook_query()))
                .header("host", "127.0.0.1")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(
        hook.status(),
        StatusCode::UNAUTHORIZED,
        "/hooks/* with a valid `hs` needs no bearer token",
    );
    // Prove the response body was consumed from the handler, not the guard.
    let _ = to_bytes(hook.into_body(), usize::MAX).await.unwrap();
}
