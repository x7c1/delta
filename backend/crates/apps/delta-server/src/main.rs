//! Delta server binary.
//!
//! A thin wrapper around [`delta_server`]: it builds configuration from the
//! environment, constructs the shared [`AppState`], and serves the [`router`] on
//! `127.0.0.1` only — Delta is a local tool and never listens on a public
//! interface. All testable logic lives in the library crate.

use std::net::{Ipv4Addr, SocketAddr};

use tracing_subscriber::EnvFilter;

use delta_server::{router, AppState};
use delta_bootstrap::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = config_from_env();

    let state = AppState::build(&config)?;

    // Continuously tail the transcript so assistant replies that Claude Code
    // flushes after the `Stop` hook still reach the browser within ~0.5s.
    state.spawn_transcript_tail();

    let app = router(state);

    // Bind to loopback only.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "delta-server listening (loopback only)");

    axum::serve(listener, app).await?;
    Ok(())
}

/// Build configuration from environment variables, with local-friendly
/// defaults so the server runs without setup during development.
///
/// The server boots fine when no tmux session exists yet: the session is created
/// lazily on the first `POST /api/sessions`, so none of these need a live session
/// at startup.
fn config_from_env() -> Config {
    Config {
        database_path: std::env::var("DELTA_DB_PATH").unwrap_or_else(|_| "delta.db".to_owned()),
        session_workdir_base: std::env::var("DELTA_SESSION_WORKDIR")
            .unwrap_or_else(|_| ".tmp/session".to_owned()),
        tmux_socket: std::env::var("DELTA_TMUX_SOCKET")
            .unwrap_or_else(|_| delta_bootstrap::DEFAULT_TMUX_SOCKET.to_owned()),
        port: env_port(),
        launch: launch_from_env(),
    }
}

fn env_port() -> u16 {
    std::env::var("DELTA_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7878)
}

/// Launch overrides from the environment, defaulting to production values.
///
/// - `DELTA_CLAUDE_BIN` substitutes the binary launched in each tmux session
///   (default `claude`). Lets tests and alternative installs supply a stand-in
///   or an out-of-`PATH` binary; the spawn command line is otherwise identical.
/// - `DELTA_LAUNCH_DEADLINE_MS` shrinks (or stretches) the launch watchdog —
///   both the unbound-fresh-spawn deadline and the resume-readiness deadline,
///   which share the same production value — so a "launch never came up" path
///   can be exercised quickly under test.
/// - `DELTA_PERMISSION_DECISION_TIMEOUT_MS` shrinks (or stretches) how long
///   the `PermissionRequest` hook response waits for a browser decision
///   before falling back to the TUI prompt, so the passthrough path can be
///   exercised quickly under test.
fn launch_from_env() -> delta_usecase::LaunchConfig {
    let mut launch = delta_usecase::LaunchConfig::default();
    if let Ok(bin) = std::env::var("DELTA_CLAUDE_BIN") {
        if !bin.is_empty() {
            launch.claude_bin = bin;
        }
    }
    if let Some(deadline) = std::env::var("DELTA_LAUNCH_DEADLINE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
    {
        launch.pending_spawn_deadline = deadline;
        launch.resume_ready_deadline = deadline;
    }
    if let Some(deadline) = std::env::var("DELTA_PERMISSION_DECISION_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
    {
        launch.permission_decision_deadline = deadline;
    }
    launch
}
