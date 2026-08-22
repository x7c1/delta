//! Router assembly: the composition root that binds a handler to every
//! endpoint `delta-wire` declares.
//!
//! What each endpoint is for — and which wire shapes it speaks — is documented
//! at the declaration in [`delta_wire::endpoint`], so this file stays a list of
//! bindings, with `RouteBinder` rejecting any drift between the two.

use axum::Router;

use delta_wire::endpoint;

use crate::api;
use crate::comms;
use crate::hooks;
use crate::pty;
use crate::route_binder::RouteBinder;
use crate::state::AppState;
use crate::ws;

/// Build the application router with all routes wired to shared state.
///
/// # Panics
///
/// If the bound routes are not exactly the declared ones — see `RouteBinder`.
pub fn router(state: AppState) -> Router {
    RouteBinder::new()
        .bind(endpoint::Health, health)
        // Control plane: Claude Code HTTP hooks.
        .bind(endpoint::HookUserPromptSubmit, hooks::user_prompt_submit)
        .bind(endpoint::HookStop, hooks::stop)
        .bind(endpoint::HookMessageDisplay, hooks::message_display)
        .bind(endpoint::HookPreToolUse, hooks::pre_tool_use)
        .bind(endpoint::HookPostToolUse, hooks::post_tool_use)
        .bind(endpoint::HookPermissionRequest, hooks::permission_request)
        .bind(endpoint::HookSessionStart, hooks::session_start)
        .bind(endpoint::HookSessionEnd, hooks::session_end)
        .bind(endpoint::HookStatusLine, hooks::status_line)
        // Browser REST surface: queries and commands.
        .bind(endpoint::ListSessions, api::list_sessions)
        .bind(endpoint::CreateSession, api::create_session)
        .bind(endpoint::OpenSession, api::open_session)
        .bind(endpoint::CloseSession, api::close_session)
        .bind(endpoint::InterruptSession, api::interrupt)
        .bind(endpoint::ListThreads, api::list_threads)
        .bind(endpoint::ListSends, api::list_sends)
        .bind(endpoint::ListThreadMessages, api::thread_messages)
        .bind(endpoint::CreateSend, api::create_send)
        .bind(endpoint::CancelSend, api::cancel_send)
        .bind(endpoint::ReleaseSend, api::release_send)
        .bind(endpoint::DecidePermission, api::decide_permission)
        .bind(endpoint::AnswerQuestion, api::answer_question)
        .bind(endpoint::CancelQuestion, api::cancel_question)
        .bind(endpoint::ListWorkdir, api::list_workdir)
        .bind(endpoint::RecentWorkdir, api::recent_workdir)
        .bind(endpoint::WorkdirGit, api::workdir_git)
        .bind(endpoint::WorkdirGitBranches, api::workdir_git_branches)
        .bind(endpoint::OpenCwd, api::open_cwd)
        .bind(endpoint::ListRepositories, api::list_repositories)
        .bind(endpoint::CloneRepository, api::clone_repository)
        .bind(endpoint::ListCloneRoots, api::list_clone_roots)
        .bind(endpoint::CreateCloneRoot, api::create_clone_root)
        .bind(endpoint::DeleteCloneRoot, api::delete_clone_root)
        .bind(endpoint::ListPullRequests, api::list_pull_requests)
        .bind(endpoint::ListProviders, api::list_providers)
        .bind(endpoint::ListLaunchOptions, api::list_launch_options)
        .bind(endpoint::CreateLaunchOption, api::create_launch_option)
        .bind(endpoint::UpdateLaunchOption, api::update_launch_option)
        .bind(endpoint::DeleteLaunchOption, api::delete_launch_option)
        .bind(endpoint::ListPromptTemplates, api::list_prompt_templates)
        .bind(endpoint::CreatePromptTemplate, api::create_prompt_template)
        .bind(endpoint::UpdatePromptTemplate, api::update_prompt_template)
        .bind(endpoint::DeletePromptTemplate, api::delete_prompt_template)
        .bind(endpoint::GetVersion, api::get_version)
        // Streams.
        .bind(endpoint::SessionEventStream, ws::ws_handler)
        .bind(endpoint::PtyStream, pty::pty_handler)
        .bind(endpoint::CommsStream, comms::comms_handler)
        .finish(state)
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
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
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn get_version_returns_a_version_string_shaped_like_v_prefixed() {
        // Smoke test the endpoint shape: the response is `{ version: "v..." }`
        // where the string starts with `v` followed by the workspace version.
        // The debug/release suffix branch is compile-time (unit-tested in
        // `crate::version`); here we just pin the JSON envelope.
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/api/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let version = body["version"].as_str().expect("version is a string");
        assert!(
            version.starts_with(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "expected the response to start with v<CARGO_PKG_VERSION>, got {version}",
        );
    }

    /// The comms-log route exists and requires a session to watch: a request
    /// without `session_id` is rejected by the query extractor before any stream
    /// is opened, so a client bug cannot leave a socket tailing nothing. (The
    /// stream's own replay-then-tail behaviour is asserted over the real stack in
    /// the Codex full-loop suite, and at the hub level in `crate::comms_log`.)
    #[tokio::test]
    async fn comms_requires_a_session_id() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/comms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn release_send_replies_conflict_with_the_stable_code_when_not_releasable() {
        // The route exists and the SendNotReleasable error surfaces as a 409
        // carrying the stable `send_not_releasable` code the frontend
        // branches on. With a fresh store no send exists, which is one of the
        // conflict cases (unknown / never-restored / already-released rows
        // all take the same guarded-UPDATE path, pinned at the store and
        // interactor levels).
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sends/9999/release")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body.get("code").and_then(serde_json::Value::as_str),
            Some("send_not_releasable"),
        );
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let response = router(test_state().await)
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
        let response = router(test_state().await)
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
    async fn workdir_list_browses_a_real_directory() {
        // Browse a temp directory containing one subdirectory and one file:
        // only the subdirectory should appear.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        std::fs::write(dir.path().join("a-file"), "x").unwrap();

        // tempdir paths contain no query-reserved characters, so they need no
        // percent-encoding for this test.
        let uri = format!("/api/workdir/list?path={}", dir.path().to_str().unwrap());
        let response = router(test_state().await)
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
        let response = router(test_state().await)
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
    async fn test_state_with_unavailable_gh() -> AppState {
        test_state_with_gh_stub().await.0
    }

    /// Like [`test_state_with_unavailable_gh`], but also hands back the counter
    /// of `clone_repo` invocations the stub has seen.
    ///
    /// The clone route's refusals are meant to start no job at all, and "no job"
    /// is only observable as "gh was never invoked" — this counter is that
    /// observation.
    async fn test_state_with_gh_stub() -> (AppState, Arc<AtomicUsize>) {
        // Mirror `test_state()`'s config exactly, then override the
        // wired Interactor's gh driver with a deterministic stub.
        struct UnavailableGh {
            clone_calls: Arc<AtomicUsize>,
        }
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
            async fn clone_repo(
                &self,
                _owner: &str,
                _name: &str,
                _destination: &str,
            ) -> delta_usecase::Result<()> {
                self.clone_calls.fetch_add(1, Ordering::SeqCst);
                // The route tests only care that a job did (or did not) start;
                // what the clone then does is the use case's own tests' subject.
                Err(delta_usecase::Error::Gh("stubbed clone".into()))
            }
        }
        let clone_calls = Arc::new(AtomicUsize::new(0));
        let gh = Arc::new(UnavailableGh {
            clone_calls: Arc::clone(&clone_calls),
        });
        let config = delta_bootstrap::Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-test-session".into(),
            worktree_base: "/tmp/delta-test-worktrees".into(),
            tmux_socket: "delta-test".into(),
            port: 7878,
            launch: delta_usecase::LaunchConfig::default(),
        };
        let interactor = delta_bootstrap::build(&config, delta_usecase::NullCommsLog::arc())
            .await
            .unwrap()
            .with_gh_cli(gh as Arc<dyn delta_usecase::GhCli>);
        (
            AppState::from_interactor(interactor, &config.tmux_socket),
            clone_calls,
        )
    }

    #[tokio::test]
    async fn prs_returns_empty_with_gh_unavailable() {
        // With the gh stub answering "unavailable", the route must
        // return 200 + `{gh_available: false, pull_requests: []}` —
        // the PR tab degrades gracefully on a host with no gh.
        let response = router(test_state_with_unavailable_gh().await)
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
        let response = router(test_state_with_unavailable_gh().await)
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
        let response = router(test_state().await)
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
        let response = router(test_state().await)
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

    /// Build a `test_state()` whose binary detector reports the Claude binary
    /// present and every other (i.e. Codex) absent, so the `/api/providers`
    /// route test is deterministic regardless of what is installed on the test
    /// host.
    async fn test_state_with_only_claude_present() -> AppState {
        use std::sync::Arc;

        struct ClaudeOnly;
        #[async_trait::async_trait]
        impl delta_usecase::BinaryDetector for ClaudeOnly {
            async fn is_available(&self, bin: &str) -> bool {
                bin == "claude"
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
        let interactor = delta_bootstrap::build(&config, delta_usecase::NullCommsLog::arc())
            .await
            .unwrap()
            .with_codex_bin("codex")
            .with_binary_detector(Arc::new(ClaudeOnly) as Arc<dyn delta_usecase::BinaryDetector>);
        AppState::from_interactor(interactor, &config.tmux_socket)
    }

    #[tokio::test]
    async fn providers_reports_availability_per_provider() {
        // 200 with both providers listed; Claude available (binary present),
        // Codex unavailable with a reason. This is the accident the endpoint
        // guards against — picking a provider whose binary is missing.
        let response = router(test_state_with_only_claude_present().await)
            .oneshot(
                Request::builder()
                    .uri("/api/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let providers = body["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 2, "both known providers are reported");

        let claude = providers
            .iter()
            .find(|p| p["provider"] == "claude")
            .expect("claude listed");
        assert_eq!(claude["available"], true);
        assert!(
            claude["detail"].is_null(),
            "available provider has no reason"
        );
        // Claude exposes an attachable terminal — the workspace shows its tab.
        assert_eq!(
            claude["capabilities"]["has_terminal"], true,
            "Claude reports a terminal"
        );
        // Claude is launched as a command line, so its launch options are argv
        // flags — Settings words its registration form from this.
        assert_eq!(
            claude["capabilities"]["launch_option_style"], "cli_flag",
            "Claude reports flag-style launch options"
        );
        // Claude's permission hook answers one request at a time — no
        // session-scoped form — so the notice must not offer that button here.
        assert_eq!(
            claude["capabilities"]["has_allow_for_session"], false,
            "Claude reports no session-scoped allow"
        );

        let codex = providers
            .iter()
            .find(|p| p["provider"] == "codex")
            .expect("codex listed");
        assert_eq!(codex["available"], false);
        assert!(
            codex["detail"].as_str().unwrap().contains("codex"),
            "unavailable provider carries a reason naming the binary"
        );
        // Codex is headless — no terminal to attach — and reports it even though
        // its binary is absent (the profile is static, not launch-dependent).
        assert_eq!(
            codex["capabilities"]["has_terminal"], false,
            "Codex reports no terminal"
        );
        // Codex is driven over a structured request, so its launch options are
        // `thread/start` field names rather than flags.
        assert_eq!(
            codex["capabilities"]["launch_option_style"], "request_field",
            "Codex reports field-style launch options"
        );
        // Codex's approval responses carry `acceptForSession`, so the notice
        // offers the session-scoped button for its sessions.
        assert_eq!(
            codex["capabilities"]["has_allow_for_session"], true,
            "Codex reports a session-scoped allow"
        );
    }

    #[tokio::test]
    async fn repositories_returns_an_empty_list_when_no_sessions() {
        // No sessions registered yet → no repositories. The endpoint
        // replies with `{ repositories: [] }`, not 404.
        let response = router(test_state().await)
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
        let response = router(test_state().await)
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
        let response = router(test_state().await)
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
        let state = test_state().await;
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
        let response = router(test_state().await)
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
    async fn prompt_templates_list_is_empty_on_a_fresh_store() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/api/prompt-templates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["prompt_templates"].as_array().unwrap().len(),
            0,
            "no templates registered yet"
        );
    }

    #[tokio::test]
    async fn create_then_list_update_and_delete_prompt_template() {
        let state = test_state().await;
        let app = router(state);

        // Create one template. The body deliberately carries newlines, which
        // must survive the round trip untouched.
        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/prompt-templates")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"label":"Merge and log","text":"\nOnce CI is green, merge.\n"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);
        let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let id = created["id"].as_i64().unwrap();
        assert_eq!(created["label"], "Merge and log");
        assert_eq!(
            created["text"], "\nOnce CI is green, merge.\n",
            "the text is stored verbatim, newlines included"
        );
        assert!(created["created_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        assert_eq!(
            created["updated_at"], created["created_at"],
            "a never-edited template reads as updated when it was created"
        );

        // It now lists.
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/prompt-templates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let listed = body["prompt_templates"].as_array().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["id"].as_i64().unwrap(), id);

        // Editing replaces both fields in place, keeping the id.
        let update = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/prompt-templates/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"label":"Merge","text":"Merge once green."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::OK);
        let bytes = to_bytes(update.into_body(), usize::MAX).await.unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(updated["id"].as_i64().unwrap(), id);
        assert_eq!(updated["label"], "Merge");
        assert_eq!(updated["text"], "Merge once green.");
        assert_eq!(
            updated["created_at"], created["created_at"],
            "an edit preserves created_at"
        );

        // The edit is reflected in the list, without adding a row.
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/prompt-templates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let listed = body["prompt_templates"].as_array().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["label"], "Merge");

        // Delete it.
        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/prompt-templates/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        // Deleting it again is an idempotent no-op, not a 404.
        let delete_again = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/prompt-templates/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_again.status(), StatusCode::NO_CONTENT);

        // The list is empty again.
        let list = app
            .oneshot(
                Request::builder()
                    .uri("/api/prompt-templates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["prompt_templates"].as_array().unwrap().len(), 0);
    }

    /// A whitespace-only `label` or `text` is a `400`: an unnamed template is
    /// unpickable and an empty one inserts nothing. The trim applies to this
    /// check only — a `text` that is *surrounded* by whitespace is accepted and
    /// stored as written (covered by the round-trip test above).
    #[tokio::test]
    async fn create_prompt_template_rejects_blank_label_or_text() {
        let app = router(test_state().await);

        for body in [
            r#"{"label":"   ","text":"some text"}"#,
            r#"{"label":"Label","text":"\n\t "}"#,
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/prompt-templates")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "expected a 400 for {body}"
            );
        }
    }

    /// Editing a template that does not exist is a `404` — unlike the delete,
    /// which is a no-op, an edit that silently hit nothing would leave the
    /// client showing content the server never stored.
    #[tokio::test]
    async fn update_prompt_template_of_an_unknown_id_is_404() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/prompt-templates/9999")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"label":"Label","text":"text"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn clone_roots_round_trip_create_list_delete() {
        let state = test_state().await;
        let app = router(state);

        // Empty on a fresh store.
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/clone-roots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["clone_roots"].as_array().unwrap().len(), 0);

        // Register one root.
        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/clone-roots")
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
                    .uri("/api/clone-roots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["clone_roots"].as_array().unwrap().len(), 1);
        assert_eq!(body["clone_roots"][0]["path"], "/home/dev/projects");

        // Duplicate is a 409 with the stable error code.
        let dup = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/clone-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"/home/dev/projects"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dup.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(dup.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "clone_root_duplicate");

        // Delete via the base64 path token.
        let token = crate::api::clone_root_path::encode("/home/dev/projects");
        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/clone-roots/{token}"))
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
                    .uri("/api/clone-roots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["clone_roots"].as_array().unwrap().len(), 0);
    }

    /// Register `path` as a clone root through the real endpoint, so the clone
    /// tests set their fixture up the same way a user would.
    async fn register_clone_root(app: &axum::Router, path: &str) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/clone-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"path":"{path}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    /// The error body's `code`, for asserting on a machine-readable refusal.
    async fn error_code(response: axum::response::Response) -> Option<String> {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        body["code"].as_str().map(str::to_owned)
    }

    #[tokio::test]
    async fn clone_repository_rejects_an_unregistered_clone_root_and_starts_no_job() {
        let (state, clone_calls) = test_state_with_gh_stub().await;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap();
        // Note: never registered. The directory existing is not enough — Delta
        // writes clones only where the user said clones go.

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/repositories/clone")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"repo_owner":"x7c1","repo_name":"delta","clone_root":"{root}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            error_code(response).await.as_deref(),
            Some("clone_root_not_registered"),
        );
        assert_eq!(
            clone_calls.load(Ordering::SeqCst),
            0,
            "a refused request must start no clone job",
        );
    }

    #[tokio::test]
    async fn clone_repository_rejects_an_existing_destination_with_409_and_starts_no_job() {
        let (state, clone_calls) = test_state_with_gh_stub().await;
        let app = router(state);
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap();
        register_clone_root(&app, root).await;
        // `<root>/delta` is taken. There is no fallback naming, so the request
        // is refused rather than landing somewhere else.
        std::fs::create_dir(tmp.path().join("delta")).unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/repositories/clone")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"repo_owner":"x7c1","repo_name":"delta","clone_root":"{root}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            error_code(response).await.as_deref(),
            Some("clone_dest_exists"),
        );
        assert_eq!(
            clone_calls.load(Ordering::SeqCst),
            0,
            "a refused request must start no clone job",
        );
    }

    #[tokio::test]
    async fn clone_repository_accepts_a_registered_root_with_202() {
        let (state, _clone_calls) = test_state_with_gh_stub().await;
        let app = router(state);
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap();
        register_clone_root(&app, root).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/repositories/clone")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"repo_owner":"x7c1","repo_name":"delta","clone_root":"{root}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Accepted, not completed: the clone outlives this response and reports
        // on `/ws`.
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn create_clone_root_rejects_a_non_absolute_path() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/clone-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"relative/path"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// A blank `path` is a `400`, whether it is empty, whitespace-only, or
    /// spelled entirely with slashes. Registering `/` by accident is not
    /// harmless: `GET /api/repositories` scans every registered root's depth-1
    /// children on every call, so a `/` row would re-read the filesystem root
    /// each time and list whichever top-level directories happen to be clones.
    #[tokio::test]
    async fn create_clone_root_rejects_a_blank_path() {
        for path in ["", "   ", "//", "///"] {
            let response = router(test_state().await)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/clone-roots")
                        .header("content-type", "application/json")
                        .body(Body::from(format!(r#"{{"path":"{path}"}}"#)))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "expected a 400 for the blank path {path:?}",
            );
        }
    }

    /// The bare root is non-blank and absolute, so rejecting blanks must not
    /// take it with them: `/` stays a registrable clone root.
    #[tokio::test]
    async fn create_clone_root_accepts_the_filesystem_root() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/clone-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"/"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["path"].as_str(), Some("/"));
    }

    /// A trailing slash is still canonicalised away, so the user-typed
    /// `/home/dev/projects/` and `/home/dev/projects` stay the same row.
    #[tokio::test]
    async fn create_clone_root_canonicalises_a_trailing_slash() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/clone-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"/home/dev/projects/"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["path"].as_str(), Some("/home/dev/projects"));
    }

    /// A whitespace-only `path` is blank, so it is refused at the query boundary
    /// rather than handed to git. (`require_path`'s trimming and its
    /// missing/empty cases are unit-tested in `crate::api`; these two tests pin
    /// that each endpoint actually routes through it.)
    #[tokio::test]
    async fn workdir_git_rejects_a_blank_path() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/api/workdir/git?path=%20%20")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn workdir_git_branches_rejects_a_blank_path() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .uri("/api/workdir/git/branches?path=%20%20")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_unknown_clone_root_is_idempotent() {
        // No registration first. The DELETE replies 204 anyway: a Settings
        // dialog click on an unknown path is the user's intent ("ensure gone"),
        // not a precondition.
        let token = crate::api::clone_root_path::encode("/never/registered");
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/clone-roots/{token}"))
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
                            .method("POST")
                            .uri("/hooks/permission-request")
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
                        .method("POST")
                        .uri("/hooks/permission-request")
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
    async fn open_cwd_rejects_a_path_not_in_the_allowlist_with_400() {
        // No sessions registered yet → no known cwds. A `POST /api/open-cwd`
        // for any path must be rejected with the stable code, and the router
        // must not have to reach the (unwired) opener stub either — the
        // allowlist check runs first.
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/open-cwd")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"/etc/passwd"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body.get("code").and_then(serde_json::Value::as_str),
            Some("open_cwd_path_not_allowed"),
        );
    }

    #[tokio::test]
    async fn open_cwd_rejects_an_unknown_handler_with_400() {
        // Register a session so the path is in the allowlist and the check
        // moves on to the handler resolution.
        let state = test_state().await;
        let app = router(state.clone());
        let submit = serde_json::json!({
            "prompt": "seed",
            "session_id": "sess-1",
            "transcript_path": "/tmp/none.jsonl",
            "cwd": "/projects/known"
        })
        .to_string();
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hooks/user-prompt-submit")
                    .header("content-type", "application/json")
                    .body(Body::from(submit))
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/open-cwd")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"path":"/projects/known","handler":"emacs"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body.get("code").and_then(serde_json::Value::as_str),
            Some("open_cwd_unknown_handler"),
        );
    }

    #[tokio::test]
    async fn open_cwd_rejects_a_blank_path_with_400() {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/open-cwd")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn permission_decision_for_an_unknown_request_is_a_conflict() {
        let response = router(test_state().await)
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
