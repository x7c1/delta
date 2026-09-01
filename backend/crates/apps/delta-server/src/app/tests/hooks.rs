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
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/user-prompt-submit{}", super::hook_query()))
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
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/user-prompt-submit{}", super::hook_query()))
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
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/pre-tool-use{}", super::hook_query()))
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
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/session-start{}", super::hook_query()))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn rejects_a_hook_without_the_secret() {
    // A forged hook POST — a local process that cleared the loopback bind and
    // the Origin/Host guard — carries no valid `hs`, so the hook auth guard must
    // refuse it with 401 before the handler runs. A valid bearer token does not
    // help: hooks authenticate through the `hs` secret, not the bearer token.
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({
        "prompt": "forged",
        "session_id": "sess-forged",
        "transcript_path": "/tmp/forged.jsonl",
        "cwd": "/work"
    })
    .to_string();

    // No `hs` at all.
    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/user-prompt-submit")
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        missing.status(),
        StatusCode::UNAUTHORIZED,
        "a hook with no `hs` secret is refused",
    );

    // A wrong `hs`.
    let wrong = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/user-prompt-submit?hs=not-the-secret")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        wrong.status(),
        StatusCode::UNAUTHORIZED,
        "a hook with the wrong `hs` secret is refused",
    );
}

#[tokio::test]
async fn rejects_a_transcript_path_outside_the_allowed_root() {
    // A hook that clears the `hs` guard but names a `transcript_path` outside the
    // configured root must be refused before the row is persisted — otherwise the
    // transcript tailer would later read (and surface) an arbitrary file. The
    // test root is `/tmp`, so a `.jsonl` under `/var/tmp` is outside it.
    let state = test_state().await;
    let app = router(state);

    let refused = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/user-prompt-submit{}", super::hook_query()))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "prompt": "hello",
                        "session_id": "sess-evil",
                        "transcript_path": "/var/tmp/outside-the-root.jsonl",
                        "cwd": "/work"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !refused.status().is_success(),
        "a transcript path outside the root must not be accepted, got {}",
        refused.status(),
    );

    // The refused hook must not have persisted a session row: nothing to read.
    let listed = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/sessions")
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let bytes = to_bytes(listed.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["sessions"].as_array().map(Vec::len),
        Some(0),
        "the refused transcript path registered no session",
    );
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
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/session-end{}", super::hook_query()))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
