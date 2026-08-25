//! Claude Code lifecycle hook routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn user_prompt_submit_hook_registers_and_responds() {
    let body = serde_json::json!({
        "prompt": "hello",
        "session_id": "sess-1",
        "transcript_path": "/tmp/does-not-exist.jsonl",
        "cwd": "/work"
    })
    .to_string();

    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hooks/user-prompt-submit")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    // No send is queued, so nothing is injected: the handler returns a
    // plain 200 with an empty body rather than a `hookSpecificOutput`.
    assert!(bytes.is_empty(), "no context to inject, so no body");
}

#[tokio::test]
async fn pre_tool_use_hook_returns_ok() {
    let body = serde_json::json!({
        "session_id": "sess-1",
        "tool_name": "Bash",
        "tool_input": {"command": "ls"},
        "tool_use_id": "toolu_01",
        "transcript_path": "/tmp/none.jsonl"
    })
    .to_string();

    // Register the session first so the foreign key is satisfied.
    let state = test_state().await;
    let app = router(state);
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hooks/user-prompt-submit")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "prompt": "seed",
                        "session_id": "sess-1",
                        "transcript_path": "/tmp/none.jsonl",
                        "cwd": "/work"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hooks/pre-tool-use")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn session_start_hook_returns_ok() {
    // A SessionStart for a session that is neither a pending spawn nor a
    // resuming session is a safe no-op: the handler emits nothing and
    // returns 200. (clear/compact and unknown ids take the same path.)
    let body = serde_json::json!({
        "session_id": "sess-1",
        "source": "startup",
        "transcript_path": "/tmp/does-not-exist.jsonl",
        "cwd": "/work"
    })
    .to_string();

    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hooks/session-start")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn session_end_hook_returns_ok() {
    // A SessionEnd for a session that is neither a pending spawn nor a known
    // session is a normal end: the handler emits nothing and returns 200.
    let body = serde_json::json!({
        "session_id": "sess-1",
        "reason": "exit"
    })
    .to_string();

    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hooks/session-end")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
