//! Permission-request hook and decision routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// Register `sess-1` through its first `UserPromptSubmit`, so the
/// permission-request hook (whose row references the session) has a
/// session row to attach to — as it always does in production.
async fn register_session(state: &AppState) {
    let body = serde_json::json!({
        "prompt": "hello",
        "session_id": "sess-1",
        "transcript_path": "/tmp/does-not-exist.jsonl",
        "cwd": "/work"
    })
    .to_string();
    let response = router(state.clone())
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
}

#[tokio::test]
async fn permission_request_hook_passes_through_on_timeout() {
    // The hook registers a pending decision and blocks; with no browser
    // decision before the (test-shortened) deadline it must answer an
    // empty 200 — the deliberate passthrough that falls back to the
    // interactive TUI prompt.
    let state = test_state().await;
    register_session(&state).await;
    let body = serde_json::json!({
        "session_id": "sess-1",
        "tool_name": "Bash",
        "tool_input": {"command": "ls"},
        "transcript_path": "/tmp/does-not-exist.jsonl"
    })
    .to_string();

    let response = router(state)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri(format!("/hooks/permission-request{}", super::hook_query()))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(
        bytes.is_empty(),
        "the passthrough must carry no decision body, got {:?}",
        String::from_utf8_lossy(&bytes)
    );
}

#[tokio::test]
async fn permission_decision_resolves_the_blocked_hook() {
    // One state shared by both requests: the hook blocks on it, the
    // decision endpoint resolves it.
    let state = test_state().await;
    register_session(&state).await;
    let hook_router = router(state.clone());
    let api_router = router(state);

    let hook_body = serde_json::json!({
        "session_id": "sess-1",
        "tool_name": "Bash",
        "tool_input": {"command": "rm -rf scratch"},
        "transcript_path": "/tmp/does-not-exist.jsonl"
    })
    .to_string();
    let hook = tokio::spawn(async move {
        hook_router
            .oneshot(
                Request::builder()
                    .header("host", "127.0.0.1")
                    .header("authorization", super::bearer())
                    .method("POST")
                    .uri(format!("/hooks/permission-request{}", super::hook_query()))
                    .header("content-type", "application/json")
                    .body(Body::from(hook_body))
                    .unwrap(),
            )
            .await
            .unwrap()
    });

    // Give the hook a beat to register its waiter, then decide. The row id
    // is 1: the in-memory store is fresh and this is its first request.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let decision = api_router
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/permissions/1/decision")
                .header("content-type", "application/json")
                .body(Body::from(r#"{ "decision": "allow" }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(decision.status(), StatusCode::NO_CONTENT);

    // The blocked hook wakes with the decision envelope.
    let response = hook.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.pointer("/hookSpecificOutput/decision/behavior")
            .and_then(serde_json::Value::as_str),
        Some("allow"),
    );
}

/// The hook envelope Claude Code receives is unchanged by the arrival of a
/// third decision variant: `allow` and `deny` still produce exactly the two
/// bodies they always did, byte for byte.
///
/// Worth pinning at the transport rather than only on the wire type, because
/// the widening moved the boolean this body is built from behind a
/// `PermissionDecision::is_allow()` call — a mistake there (folding the new
/// variant into `deny`, say) would be invisible to a type that only ever
/// sees the boolean.
#[tokio::test]
async fn the_claude_hook_envelope_is_unchanged_for_allow_and_deny() {
    for (decision, expected_behavior) in [("allow", "allow"), ("deny", "deny")] {
        let state = test_state().await;
        register_session(&state).await;
        let hook_router = router(state.clone());
        let api_router = router(state);

        let hook_body = serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "transcript_path": "/tmp/does-not-exist.jsonl"
        })
        .to_string();
        let hook = tokio::spawn(async move {
            hook_router
                .oneshot(
                    Request::builder()
                        .header("host", "127.0.0.1")
                        .header("authorization", super::bearer())
                        .method("POST")
                        .uri(format!("/hooks/permission-request{}", super::hook_query()))
                        .header("content-type", "application/json")
                        .body(Body::from(hook_body))
                        .unwrap(),
                )
                .await
                .unwrap()
        });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let response = api_router
            .oneshot(
                Request::builder()
                    .header("host", "127.0.0.1")
                    .header("authorization", super::bearer())
                    .method("POST")
                    .uri("/api/permissions/1/decision")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "decision": decision }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = hook.await.unwrap();
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": expected_behavior },
                }
            }),
            "the `{decision}` hook envelope changed"
        );
    }
}

/// A session-scoped allow posted against a provider that does not declare
/// the capability is refused with the documented `400` and its stable code —
/// not a `500`, and not a silent downgrade to a plain allow, which would keep
/// prompting a user who asked to stop being prompted.
///
/// The refusal is inert, which is the part worth a transport-level test: the
/// blocked hook is still blocked afterwards (nothing it cannot express was
/// handed to it), and the same request still answers to a plain allow — so a
/// mis-aimed click cannot strand a live prompt behind a spurious conflict.
#[tokio::test]
async fn a_session_scoped_decision_is_refused_for_a_provider_without_the_capability() {
    let state = test_state().await;
    register_session(&state).await;
    let hook_router = router(state.clone());
    let api_router = router(state);

    let hook_body = serde_json::json!({
        "session_id": "sess-1",
        "tool_name": "Bash",
        "tool_input": {"command": "ls"},
        "transcript_path": "/tmp/does-not-exist.jsonl"
    })
    .to_string();
    let hook = tokio::spawn(async move {
        hook_router
            .oneshot(
                Request::builder()
                    .header("host", "127.0.0.1")
                    .header("authorization", super::bearer())
                    .method("POST")
                    .uri(format!("/hooks/permission-request{}", super::hook_query()))
                    .header("content-type", "application/json")
                    .body(Body::from(hook_body))
                    .unwrap(),
            )
            .await
            .unwrap()
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let response = api_router
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/permissions/1/decision")
                .header("content-type", "application/json")
                .body(Body::from(r#"{ "decision": "allow_for_session" }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the decision value is wrong for this provider, not the request state"
    );
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.get("code").and_then(serde_json::Value::as_str),
        Some("permission_decision_unsupported"),
    );

    // Nothing reached the agent: the hook is still waiting. (Its deadline is
    // test-shortened, so a finished task here would mean it was answered.)
    assert!(
        !hook.is_finished(),
        "the blocked hook must not have been answered"
    );

    // And the very same request is still answerable with a decision this
    // provider does have.
    let response = api_router
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/permissions/1/decision")
                .header("content-type", "application/json")
                .body(Body::from(r#"{ "decision": "allow" }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = hook.await.unwrap();
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.pointer("/hookSpecificOutput/decision/behavior")
            .and_then(serde_json::Value::as_str),
        Some("allow"),
    );
}

#[tokio::test]
async fn permission_decision_for_an_unknown_request_is_a_conflict() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/permissions/999/decision")
                .header("content-type", "application/json")
                .body(Body::from(r#"{ "decision": "deny" }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.get("code").and_then(serde_json::Value::as_str),
        Some("permission_not_pending"),
    );
}
