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
        // A tool call completed; used to close a subagent's running window.
        .route("/hooks/post-tool-use", post(hooks::post_tool_use))
        // Interactive permission dialog appeared (a human answer is pending).
        .route("/hooks/permission-request", post(hooks::permission_request))
        // A session's TUI became ready (launch-readiness signal): binds a fresh
        // spawn on startup, releases the held first prompt on resume.
        .route("/hooks/session-start", post(hooks::session_start))
        // A session terminated; used to catch a spawn that died before binding.
        .route("/hooks/session-end", post(hooks::session_end))
        // The latest Claude Code status-line snapshot (model / context-window
        // usage / rate limits / cost), broadcast to the browser. None of this
        // is in the transcript, so the snapshot is the only source for it. Not
        // a hook: it is the `statusLine` command Delta injects into the session
        // settings.
        .route("/hooks/status-line", post(hooks::status_line))
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
        // Cancel a still-queued send before it is dispatched into the pane.
        .route("/api/sends/{id}/cancel", post(api::cancel_send))
        // Answer a pending tool-permission request from the browser.
        .route(
            "/api/permissions/{id}/decision",
            post(api::decide_permission),
        )
        // Answer a pending AskUserQuestion from the browser (keystroke injection
        // into the session's TUI pane).
        .route(
            "/api/sessions/{id}/questions/{request_id}/answer",
            post(api::answer_question),
        )
        // Cancel a pending AskUserQuestion from the browser (Escape injection
        // into the session's TUI pane). The request_id rides in the body since
        // a cancel carries no selection.
        .route(
            "/api/sessions/{id}/questions/cancel",
            post(api::cancel_question),
        )
        // Working-directory picker: browse and recents (read-only).
        .route("/api/workdir/list", get(api::list_workdir))
        .route("/api/workdir/recent", get(api::recent_workdir))
        // Registered repositories for the new-session Repository tab: every
        // distinct repo Delta has launched a session under, with its known
        // clones bundled by origin URL and ordered by recency.
        .route("/api/repositories", get(api::list_repositories))
        // Repository scan roots: parent directories whose direct children
        // every `/api/repositories` call probes for git clones, surfacing
        // clones the user has never launched a session in (the umbrella-
        // session pattern). The registered path is URL-safe base64 in the
        // DELETE path segment so its embedded `/` characters survive routing.
        .route(
            "/api/repository-scan-roots",
            get(api::list_repository_scan_roots).post(api::create_repository_scan_root),
        )
        .route(
            "/api/repository-scan-roots/{path_b64}",
            axum::routing::delete(api::delete_repository_scan_root),
        )
        // Pull requests for the new-session PR tab (per lens): drives
        // `gh search prs` through the gh CLI gateway and tags each row
        // with whether Delta has a local clone of the PR's repo.
        .route("/api/prs", get(api::list_pull_requests))
        // Git detection for the worktree-at-start option (read-only): is the
        // selected directory a git repo, and what remote branches can a worktree
        // be based on.
        .route("/api/workdir/git", get(api::workdir_git))
        .route("/api/workdir/git/branches", get(api::workdir_git_branches))
        // Launch-option registry: list, create, update (toggle the
        // `default_enabled` flag), and delete the custom `claude` CLI flags the
        // user can later select when starting a session.
        .route(
            "/api/launch-options",
            get(api::list_launch_options).post(api::create_launch_option),
        )
        .route(
            "/api/launch-options/{id}",
            axum::routing::patch(api::update_launch_option)
                .delete(api::delete_launch_option),
        )
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
            worktree_base: "/tmp/delta-test-worktrees".into(),
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
        // No send is queued, so nothing is injected: the handler returns a
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

    /// Build a `test_state()` whose gh CLI is stubbed to report
    /// "unavailable", so the PR-route smoke tests are independent of
    /// whether `gh` happens to be installed on the test host.
    fn test_state_with_unavailable_gh() -> AppState {
        // Mirror `test_state()`'s config exactly, then override the
        // wired Interactor's gh driver with a deterministic stub.
        use std::sync::Arc;

        struct UnavailableGh;
        #[async_trait::async_trait]
        impl delta_usecase::GhCli for UnavailableGh {
            async fn is_authenticated(&self) -> bool {
                false
            }
            async fn search_prs(
                &self,
                _lens: delta_usecase::PullRequestLens,
            ) -> delta_usecase::Result<Vec<delta_usecase::PullRequest>> {
                Ok(Vec::new())
            }
        }
        let config = delta_bootstrap::Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-test-session".into(),
            worktree_base: "/tmp/delta-test-worktrees".into(),
            tmux_socket: "delta-test".into(),
            port: 7878,
            launch: delta_usecase::LaunchConfig::default(),
        };
        let interactor = delta_bootstrap::build(&config)
            .unwrap()
            .with_gh_cli(Arc::new(UnavailableGh) as Arc<dyn delta_usecase::GhCli>);
        AppState::from_interactor(interactor, &config.tmux_socket)
    }

    #[tokio::test]
    async fn prs_returns_empty_with_gh_unavailable() {
        // With the gh stub answering "unavailable", the route must
        // return 200 + `{gh_available: false, pull_requests: []}` —
        // the PR tab degrades gracefully on a host with no gh.
        let response = router(test_state_with_unavailable_gh())
            .oneshot(
                Request::builder()
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
        let response = router(test_state_with_unavailable_gh())
            .oneshot(
                Request::builder()
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
        let response = router(test_state())
            .oneshot(
                Request::builder()
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
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .uri("/api/prs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn repositories_returns_an_empty_list_when_no_sessions() {
        // No sessions registered yet → no repositories. The endpoint
        // replies with `{ repositories: [] }`, not 404.
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .uri("/api/repositories")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["repositories"].as_array().unwrap().len(),
            0,
            "no sessions = no repositories"
        );
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
    async fn launch_options_list_is_empty_on_a_fresh_store() {
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .uri("/api/launch-options")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["launch_options"].as_array().unwrap().len(),
            0,
            "no options registered yet"
        );
    }

    #[tokio::test]
    async fn create_then_list_and_delete_launch_option() {
        let state = test_state();
        let app = router(state);

        // Create one option.
        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/launch-options")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"label":"plugins","name":"--plugin-dir","value":"/opt/p"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);
        let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let id = created["id"].as_i64().unwrap();
        assert_eq!(created["name"], "--plugin-dir");

        // It now lists.
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/launch-options")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["launch_options"].as_array().unwrap().len(), 1);

        // Delete it.
        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/launch-options/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        // The list is empty again.
        let list = app
            .oneshot(
                Request::builder()
                    .uri("/api/launch-options")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["launch_options"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_launch_option_rejects_a_blank_name() {
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/launch-options")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn repository_scan_roots_round_trip_create_list_delete() {
        let state = test_state();
        let app = router(state);

        // Empty on a fresh store.
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/repository-scan-roots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["scan_roots"].as_array().unwrap().len(), 0);

        // Register one root.
        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/repository-scan-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"/home/dev/projects/"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);
        let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Trailing slash is trimmed for canonicalisation.
        assert_eq!(created["path"], "/home/dev/projects");

        // Listed.
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/repository-scan-roots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["scan_roots"].as_array().unwrap().len(), 1);
        assert_eq!(body["scan_roots"][0]["path"], "/home/dev/projects");

        // Duplicate is a 409 with the stable error code.
        let dup = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/repository-scan-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"/home/dev/projects"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dup.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(dup.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "scan_root_duplicate");

        // Delete via the base64 path token.
        let token = crate::api::repository_scan_root_path::encode("/home/dev/projects");
        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/repository-scan-roots/{token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        // The list is empty again.
        let list = app
            .oneshot(
                Request::builder()
                    .uri("/api/repository-scan-roots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["scan_roots"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_repository_scan_root_rejects_a_non_absolute_path() {
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/repository-scan-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"relative/path"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_unknown_repository_scan_root_is_idempotent() {
        // No registration first. The DELETE replies 204 anyway: a Settings
        // dialog click on an unknown path is the user's intent ("ensure gone"),
        // not a precondition.
        let token = crate::api::repository_scan_root_path::encode("/never/registered");
        let response = router(test_state())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/repository-scan-roots/{token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
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
            "tool_input": {"command": "ls"},
            "transcript_path": "/tmp/does-not-exist.jsonl"
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
            "tool_input": {"command": "rm -rf scratch"},
            "transcript_path": "/tmp/does-not-exist.jsonl"
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
    async fn status_line_post_broadcasts_a_status_updated_event() {
        // The API-response-present shape: rate_limits present and
        // context_window.used_percentage populated. The handler must broadcast
        // a `StatusUpdated` carrying the session id, the forwarded
        // used_percentage, and both rate-limit windows.
        let state = test_state();
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
                assert_eq!(snapshot.context_used_percentage, Some(42.5));
                // `current_usage` arrives as an object; the snapshot sums its
                // input-side buckets (5000 + 0 + 80000) into the occupancy.
                assert_eq!(snapshot.context_current_usage, Some(85000));
                let five_hour = snapshot.five_hour.expect("5h window present");
                assert_eq!(five_hour.used_percentage, Some(12.0));
                assert_eq!(five_hour.resets_at, Some(1700000000));
                let seven_day = snapshot.seven_day.expect("7d window present");
                assert_eq!(seven_day.used_percentage, Some(3.5));
                assert_eq!(seven_day.resets_at, Some(1700500000));
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
        let state = test_state();
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
                assert!(snapshot.five_hour.is_none(), "rate limits absent");
                assert!(snapshot.seven_day.is_none(), "rate limits absent");
            }
            other => panic!("expected StatusUpdated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn status_line_tolerates_an_unknown_top_level_field() {
        // Claude Code adds fields across versions; an unknown extra top-level
        // field must not break deserialization (forward compatibility).
        let state = test_state();
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
        let state = test_state();
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
