//! The create-session endpoint.

use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{build_app, post_json};

#[tokio::test]
async fn create_session_endpoint_reports_starting_then_ready() {
    let (app, tmux, transcript_path, _state) = build_app();

    // No session exists yet, so the first POST /api/sessions spawns one and the
    // route serializes the lifecycle as "starting".
    let (status, body) = post_json(&app, "/api/sessions", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["status"], "starting",
        "a freshly spawned session is starting"
    );
    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        1,
        "exactly one session was spawned"
    );

    // A second POST finds a live spawn already in the registry: idempotent
    // reuse, reported as "ready" with no second spawn.
    let (status, body) = post_json(&app, "/api/sessions", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready", "a reused session is ready");
    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        1,
        "no second session was spawned"
    );

    let _ = std::fs::remove_file(&transcript_path);
}
