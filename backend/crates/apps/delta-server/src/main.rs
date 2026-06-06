//! Delta server binary.
//!
//! A thin wrapper around [`delta_server`]: it builds configuration from the
//! environment, constructs the shared [`AppState`], and serves the [`router`] on
//! `127.0.0.1` only — Delta is a local tool and never listens on a public
//! interface. All testable logic lives in the library crate.

use std::net::{Ipv4Addr, SocketAddr};

use tracing_subscriber::EnvFilter;

use delta_server::{router, AppState};
use delta_wire::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = config_from_env();
    let port = env_port();

    let state = AppState::build(&config)?;

    // Continuously tail the transcript so assistant replies that Claude Code
    // flushes after the `Stop` hook still reach the browser within ~0.5s.
    state.spawn_transcript_tail();

    let app = router(state);

    // Bind to loopback only.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "delta-server listening (loopback only)");

    axum::serve(listener, app).await?;
    Ok(())
}

/// Build configuration from environment variables, with local-friendly
/// defaults so the server runs without setup during development.
fn config_from_env() -> Config {
    Config {
        database_path: std::env::var("DELTA_DB_PATH").unwrap_or_else(|_| "delta.db".to_owned()),
        tmux_pane: std::env::var("DELTA_TMUX_PANE").unwrap_or_else(|_| "delta:0.0".to_owned()),
    }
}

fn env_port() -> u16 {
    std::env::var("DELTA_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7878)
}
