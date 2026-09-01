//! Provider availability route.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

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
                .header("host", "127.0.0.1")
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
