//! Router assembly.

use axum::routing::{get, post};
use axum::Router;

use crate::api;
use crate::hooks;
use crate::pty;
use crate::state::AppState;
use crate::ws;

/// Build the application router with all routes wired to shared state.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        // Control plane: Claude Code HTTP hooks.
        .route("/hooks/user-prompt-submit", post(hooks::user_prompt_submit))
        .route("/hooks/stop", post(hooks::stop))
        // Live assistant text streamed during generation (before the transcript
        // flush); buffered as a provisional preview and broadcast to the browser.
        .route("/hooks/message-display", post(hooks::message_display))
        .route("/hooks/pre-tool-use", post(hooks::pre_tool_use))
        // Interactive permission dialog appeared (a human answer is pending).
        .route("/hooks/permission-request", post(hooks::permission_request))
        // A session's TUI became ready (launch-readiness signal): binds a fresh
        // spawn on startup, releases the held first prompt on resume.
        .route("/hooks/session-start", post(hooks::session_start))
        // A session terminated; used to catch a spawn that died before binding.
        .route("/hooks/session-end", post(hooks::session_end))
        // Browser REST surface: queries and commands.
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/{id}/open", post(api::open_session))
        .route("/api/sessions/{id}/close", post(api::close_session))
        .route("/api/sessions/{id}/threads", get(api::list_threads))
        .route("/api/sessions/{id}/sends", get(api::list_sends))
        .route("/api/threads/{id}/messages", get(api::thread_messages))
        .route("/api/sends", post(api::create_send))
        // Answer a pending tool-permission request from the browser.
        .route(
            "/api/permissions/{id}/decision",
            post(api::decide_permission),
        )
        // Working-directory picker: browse and recents (read-only).
        .route("/api/workdir/list", get(api::list_workdir))
        .route("/api/workdir/recent", get(api::recent_workdir))
        // Browser event stream.
        .route("/ws", get(ws::ws_handler))
        // Terminal bridge to the tmux pane.
        .route("/pty", get(pty::pty_handler))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use delta_bootstrap::Config;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::build(&Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-test-session".into(),
            tmux_socket: "delta-test".into(),
            port: 7878,
            launch: delta_usecase::LaunchConfig {
                // The permission-request hook test exercises the no-decision
                // passthrough, which waits out this deadline; keep it short.
                permission_decision_deadline: std::time::Duration::from_millis(50),
                ..delta_usecase::LaunchConfig::default()
            },
        })
        .unwrap()
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let response = router(test_state())
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
    async fn list_sessions_rejects_a_malformed_cursor() {
        // A non-decodable cursor is a client error, surfaced as 400 rather than
        // silently ignored or treated as the first page.
        let response = router(test_state())
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

    #[tokio::test]
    async fn user_prompt_submit_hook_registers_and_responds() {
        let body = serde_json::json!({
            "prompt": "hello",
            "session_id": "sess-1",
            "transcript_path": "/tmp/does-not-exist.jsonl",
            "cwd": "/work"
        })
        .to_string();

        let response = router(test_state())
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
        // No pending send queued, so nothing is injected: the handler returns a
        // plain 200 with an empty body rather than a `hookSpecificOutput`.
        assert!(bytes.is_empty(), "no context to inject, so no body");
    }

    #[tokio::test]
    async fn workdir_list_browses_a_real_directory() {
        // Browse a temp directory containing one subdirectory and one file:
        // only the subdirectory should appear.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        std::fs::write(dir.path().join("a-file"), "x").unwrap();

        // tempdir paths contain no query-reserved characters, so they need no
        // percent-encoding for this test.
        let uri = format!("/api/workdir/list?path={}", dir.path().to_str().unwrap());
        let response = router(test_state())
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "dirs only, files excluded");
        assert_eq!(entries[0]["name"], "child");
        assert!(body["parent"].is_string(), "a non-root dir has a parent");
    }

    #[tokio::test]
    async fn workdir_list_rejects_a_missing_path_with_400() {
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .uri("/api/workdir/list?path=/no/such/path/here")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn workdir_recent_returns_an_empty_list_when_no_sessions() {
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .uri("/api/workdir/recent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["workdirs"].as_array().unwrap().len(),
            0,
            "no sessions yet means no recent workdirs"
        );
    }

    #[tokio::test]
    async fn pre_tool_use_hook_returns_ok() {
        let body = serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "tool_use_id": "toolu_01"
        })
        .to_string();

        // Register the session first so the foreign key is satisfied.
        let state = test_state();
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

        let response = router(test_state())
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

        let response = router(test_state())
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
                    .method("POST")
                    .uri("/hooks/user-prompt-submit")
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
        let state = test_state();
        register_session(&state).await;
        let body = serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": "ls"}
        })
        .to_string();

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/permission-request")
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
        let state = test_state();
        register_session(&state).await;
        let hook_router = router(state.clone());
        let api_router = router(state);

        let hook_body = serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf scratch"}
        })
        .to_string();
        let hook = tokio::spawn(async move {
            hook_router
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/hooks/permission-request")
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

    #[tokio::test]
    async fn permission_decision_for_an_unknown_request_is_a_conflict() {
        let response = router(test_state())
            .oneshot(
                Request::builder()
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
}
