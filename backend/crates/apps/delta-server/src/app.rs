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
        .route("/hooks/pre-tool-use", post(hooks::pre_tool_use))
        // Browser REST surface: queries and commands.
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/{id}/open", post(api::open_session))
        .route("/api/sessions/{id}/close", post(api::close_session))
        .route("/api/sessions/{id}/threads", get(api::list_threads))
        .route("/api/threads/{id}/messages", get(api::thread_messages))
        .route("/api/sends", post(api::create_send))
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
    use delta_wire::Config;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::build(&Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-test-session".into(),
            tmux_socket: "delta-test".into(),
            port: 7878,
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
    async fn pre_tool_use_hook_returns_ok() {
        let body = serde_json::json!({
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": "ls"}
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
}
