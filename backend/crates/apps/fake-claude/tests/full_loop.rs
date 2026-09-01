//! Full-stack loop test: a real `delta-server` spawning this crate's
//! `fake-claude` binary through real tmux.
//!
//! The test boots the production server wiring (SQLite store, JSONL
//! transcript reader, tmux driver) on a random port with `claude_bin` pointed
//! at the built `fake-claude`, drives `POST /api/sends` to start a new
//! session, and asserts over REST that the whole loop closed: the session
//! registered via the hooks the fake fired, and the scripted user/assistant
//! messages landed in the thread via the transcript tail.
//!
//! Requires a `tmux` on `PATH`; the test skips (with a note) where tmux is
//! absent so the workspace test suite stays runnable everywhere. CI installs
//! tmux explicitly so the loop is always exercised there.

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use delta_bootstrap::{Config, LaunchConfig};
use delta_server::{router, AppState};

/// How long the test waits for an expected observable state. Generous: a
/// healthy run completes in a couple of seconds; the deadline only bounds a
/// genuinely broken run.
const WAIT_DEADLINE: Duration = Duration::from_secs(20);

/// Poll interval between REST probes.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The bearer token the assembled backend holds, presented on every request so
/// the loop clears the router's per-run auth guard.
const AUTH_TOKEN: &str = "delta-fake-claude-full-loop-auth-token";

/// The hook secret the assembled backend holds. It is rendered into the hook
/// URLs the fake reads from the settings file, so the fake's hook callbacks carry
/// it and clear the hook auth guard with no fake-side change.
const HOOK_SECRET: &str = "delta-fake-claude-full-loop-hook-secret";

#[tokio::test]
async fn a_new_session_send_round_trips_through_tmux_and_the_fake_binary() {
    if !tmux_available() {
        eprintln!("skipping: tmux is not available on PATH");
        return;
    }

    let temp = tempfile::tempdir().expect("create temp dir");
    let scenario_path = temp.path().join("scenario.json");
    std::fs::write(
        &scenario_path,
        r#"{
            "steps": [
                { "type": "await_prompt" },
                { "type": "reply", "text": "scripted reply", "thinking": "scripted thinking" },
                { "type": "stop" }
            ]
        }"#,
    )
    .expect("write scenario");

    // The spawn command line is fixed (`<bin> --settings … --session-id …`),
    // so per-run configuration reaches the fake through a wrapper script that
    // pins its environment. This also keeps the test independent of the
    // environment the tmux server captured at boot.
    // The fake writes its transcript under this directory, so the server must
    // accept transcript paths rooted here — point `DELTA_TRANSCRIPT_ROOT` (via
    // `Config::transcript_root`) at the same place the fake writes.
    let transcript_dir = temp.path().join("transcripts");
    let claude_bin = write_wrapper_script(
        temp.path(),
        &scenario_path.to_string_lossy(),
        &transcript_dir.to_string_lossy(),
    );

    // A unique socket per run: parallel or leftover runs never collide, and
    // teardown can kill the whole tmux server without touching anything else.
    let tmux_socket = format!("delta-fake-test-{}", std::process::id());

    // Bind the hook listener first so the rendered settings carry the real port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind hook listener");
    let port = listener.local_addr().expect("local addr").port();

    let config = Config {
        database_path: temp.path().join("delta.db").to_string_lossy().into_owned(),
        session_workdir_base: temp.path().join("workdirs").to_string_lossy().into_owned(),
        worktree_base: temp.path().join("worktrees").to_string_lossy().into_owned(),
        tmux_socket: tmux_socket.clone(),
        auth_token: AUTH_TOKEN.into(),
        hook_secret: HOOK_SECRET.into(),
        transcript_root: transcript_dir.to_string_lossy().into_owned(),
        port,
        launch: LaunchConfig {
            claude_bin: claude_bin.to_string_lossy().into_owned(),
            ..LaunchConfig::default()
        },
    };
    let state = AppState::build(&config).await.expect("build app state");
    // The tail is what ingests the assistant lines the fake writes after the
    // Stop hook — without it only hook-triggered syncs would run.
    let tail = state.spawn_transcript_tail();
    let app = router(state);

    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, server_app).await.expect("serve");
    });

    let result = drive_loop(&app, temp.path()).await;

    // Teardown before asserting, so a failed assertion never leaks the pane.
    let _ = std::process::Command::new("tmux")
        .args(["-L", &tmux_socket, "kill-server"])
        .output();
    server.abort();
    tail.abort();

    result.expect("full loop");
}

/// Drive the loop and return a readable error instead of panicking, so the
/// caller can tear the tmux server down first.
async fn drive_loop(app: &axum::Router, workdir: &std::path::Path) -> Result<(), String> {
    // Start a new session with a first prompt, in an existing directory.
    let send_body = serde_json::json!({
        "new_session": true,
        "text": "hello fake claude",
        "workdir": workdir.to_string_lossy(),
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/sends")
                // The router's Origin/Host guard rejects any non-loopback Host
                // with 403, so every request that drives it needs a loopback Host.
                .header("host", "127.0.0.1")
                // A valid bearer token clears the per-run auth guard.
                .header("authorization", format!("Bearer {AUTH_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(send_body.to_string()))
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    if response.status() != StatusCode::CREATED {
        return Err(format!("POST /api/sends returned {}", response.status()));
    }

    // The session registers once the fake's SessionStart/UserPromptSubmit
    // hooks land; its id is whatever Delta minted, so discover it by polling
    // the list.
    let main_thread_id = wait_for(app, "/api/sessions", |body| {
        let sessions = body["sessions"].as_array()?;
        let item = sessions.first()?;
        item["main_thread_id"].as_i64()
    })
    .await?;

    // The scripted turn lands via transcript ingestion: the user line and the
    // assistant reply both attributed to `main`.
    wait_for(
        app,
        &format!("/api/threads/{main_thread_id}/messages"),
        |body| {
            let messages = body["messages"].as_array()?;
            let has_user = messages
                .iter()
                .any(|m| m["role"] == "user" && m["content_text"] == "hello fake claude");
            let has_assistant = messages.iter().any(|m| m["role"] == "assistant");
            (has_user && has_assistant).then_some(())
        },
    )
    .await?;
    Ok(())
}

/// Poll `path` until `extract` returns `Some`, or fail after [`WAIT_DEADLINE`].
async fn wait_for<T>(
    app: &axum::Router,
    path: &str,
    extract: impl Fn(&serde_json::Value) -> Option<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + WAIT_DEADLINE;
    let mut last_body = serde_json::Value::Null;
    while Instant::now() < deadline {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("host", "127.0.0.1")
                    .header("authorization", format!("Bearer {AUTH_TOKEN}"))
                    .body(Body::empty())
                    .map_err(|e| e.to_string())?,
            )
            .await
            .map_err(|e| e.to_string())?;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .map_err(|e| e.to_string())?;
        last_body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        if let Some(value) = extract(&last_body) {
            return Ok(value);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(format!(
        "timed out waiting on {path}; last body: {last_body}"
    ))
}

/// Whether a usable `tmux` is on `PATH`.
fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Write the executable wrapper that launches the built `fake-claude` with a
/// pinned scenario and transcript directory, forwarding the CLI args.
fn write_wrapper_script(
    dir: &std::path::Path,
    scenario: &str,
    transcript_dir: &str,
) -> std::path::PathBuf {
    let path = dir.join("claude-wrapper.sh");
    let script = format!(
        "#!/bin/sh\n\
         FAKE_CLAUDE_SCENARIO='{scenario}' \
         FAKE_CLAUDE_TRANSCRIPT_DIR='{transcript_dir}' \
         exec '{bin}' \"$@\"\n",
        bin = env!("CARGO_BIN_EXE_fake-claude"),
    );
    std::fs::write(&path, script).expect("write wrapper script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod wrapper script");
    }
    path
}
