//! The status-line hook route.

use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn status_line_post_broadcasts_a_status_updated_event() {
    // The API-response-present shape: rate_limits present and
    // context_window.used_percentage populated. The handler must broadcast
    // a `StatusUpdated` carrying the session id, the forwarded
    // used_percentage, and both rate-limit windows.
    let state = test_state().await;
    let mut rx = state.subscribe();
    let app = router(state);

    let body = serde_json::json!({
        "session_id": "sess-status",
        "model": { "id": "claude-opus-4", "display_name": "Opus 4" },
        "context_window": {
            "used_percentage": 42.5,
            "context_window_size": 200000,
            "current_usage": {
                "input_tokens": 5000,
                "output_tokens": 200,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 80000
            },
            "total_input_tokens": 90000
        },
        "rate_limits": {
            "five_hour": { "used_percentage": 12.0, "resets_at": 1700000000 },
            "seven_day": { "used_percentage": 3.5, "resets_at": 1700500000 }
        },
        "cost": { "total_cost_usd": 0.1234 },
        "workspace": { "current_dir": "/work" },
        "fast_mode": false
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/status-line")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let event = rx.try_recv().expect("a StatusUpdated event was broadcast");
    match event {
        delta_usecase::SessionEvent::StatusUpdated {
            session_id,
            snapshot,
        } => {
            assert_eq!(session_id.0, "sess-status");
            assert_eq!(
                snapshot.provider,
                delta_usecase::AgentProvider::Claude,
                "the status line is Claude's edge, and the snapshot says so"
            );
            assert_eq!(snapshot.context_used_percentage, Some(42.5));
            // `current_usage` arrives as an object; the snapshot sums its
            // input-side buckets (5000 + 0 + 80000) into the occupancy.
            assert_eq!(snapshot.context_current_usage, Some(85000));
            // Claude's two named windows become duration-identified ones, in
            // significance order: the 5-hour window first, the 7-day second.
            let windows = snapshot.rate_limits.expect("rate limits stated");
            assert_eq!(
                windows,
                vec![
                    delta_usecase::RateLimitWindow {
                        duration_seconds: Some(5 * 60 * 60),
                        used_percentage: Some(12.0),
                        resets_at: Some(1700000000),
                    },
                    delta_usecase::RateLimitWindow {
                        duration_seconds: Some(7 * 24 * 60 * 60),
                        used_percentage: Some(3.5),
                        resets_at: Some(1700500000),
                    },
                ]
            );
        }
        other => panic!("expected StatusUpdated, got {other:?}"),
    }
}

#[tokio::test]
async fn status_line_pre_api_shape_deserializes_with_all_optionals_absent() {
    // Before the first API response, `rate_limits` is absent entirely and
    // `context_window.current_usage` / `used_percentage` are null. The
    // payload must deserialize (every field optional) and still broadcast a
    // snapshot with those fields as `None`.
    let state = test_state().await;
    let mut rx = state.subscribe();
    let app = router(state);

    let body = serde_json::json!({
        "session_id": "sess-status",
        "model": { "id": "claude-opus-4", "display_name": "Opus 4" },
        "context_window": {
            "used_percentage": null,
            "context_window_size": 200000,
            "current_usage": null,
            "total_input_tokens": 0
        },
        "cost": { "total_cost_usd": 0.0 },
        "workspace": { "current_dir": "/work" }
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/status-line")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let event = rx.try_recv().expect("a StatusUpdated event was broadcast");
    match event {
        delta_usecase::SessionEvent::StatusUpdated { snapshot, .. } => {
            assert_eq!(snapshot.context_used_percentage, None);
            assert_eq!(snapshot.context_current_usage, None);
            // The status line always states the account's rate limits, so an
            // absent `rate_limits` section is an empty list ("this account
            // has none") rather than silence — a subscription that lapsed
            // must clear the footer rows, not freeze them.
            assert_eq!(
                snapshot.rate_limits,
                Some(Vec::new()),
                "rate limits stated as none"
            );
        }
        other => panic!("expected StatusUpdated, got {other:?}"),
    }
}

#[tokio::test]
async fn status_line_tolerates_an_unknown_top_level_field() {
    // Claude Code adds fields across versions; an unknown extra top-level
    // field must not break deserialization (forward compatibility).
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({
        "session_id": "sess-status",
        "model": { "id": "claude-opus-4", "display_name": "Opus 4" },
        "some_future_field": { "nested": [1, 2, 3] }
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/status-line")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_line_without_a_session_id_is_dropped_with_no_event() {
    // `session_id` is optional in the upstream schema, and a snapshot is
    // keyed by it: a payload missing it carries nothing to broadcast on, so
    // the handler drops it (empty 200) rather than emitting a `StatusUpdated`
    // with no session to attach to.
    let state = test_state().await;
    let mut rx = state.subscribe();
    let app = router(state);

    let body = serde_json::json!({
        "model": { "id": "claude-opus-4", "display_name": "Opus 4" },
        "context_window": { "used_percentage": 42.5 }
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/status-line")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        rx.try_recv().is_err(),
        "a session-less status line carries nothing to broadcast"
    );
}
