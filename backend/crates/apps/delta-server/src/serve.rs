//! Starting the server: logging, the loopback listener, the startup errors a
//! user has to act on, and serving the [`router`] on a bound listener.
//!
//! Shared by the `delta-server` binary and the desktop shell (`delta-app`) so
//! both start the server the same way. Delta is a local tool and never listens
//! on a public interface, so the only listener offered here is a loopback one.

use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::{router, AppState};

/// Install the global `tracing` subscriber: `RUST_LOG` when set, `info`
/// otherwise.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

/// Bind `127.0.0.1:<port>`. Port `0` takes a free port; read it back from
/// [`TcpListener::local_addr`].
pub async fn bind_loopback(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// The message for a startup failure the user — not Delta — has to act on, or
/// `None` for any other error, which keeps its default propagation.
///
/// A refused overlay is one (the remediation is `make reset`). The migration
/// ladder migrates an out-of-date database forward on its own, so what reaches
/// here is only what it cannot fix: a database written by a newer binary, one
/// that predates the version stamp entirely, or one stamped below the ladder's
/// squashed baseline. The inner store error is returned verbatim — its
/// `Display` already names the remediation.
///
/// A missing host command is the other. Its own `Display` names the command, so
/// that line is returned as-is: installing tmux is the user's business and Delta
/// has no advice to give about how.
pub fn user_facing_startup_error(err: &anyhow::Error) -> Option<String> {
    match err.downcast_ref::<delta_bootstrap::Error>()? {
        delta_bootstrap::Error::Store(
            store_err @ (delta_bootstrap::StoreError::SchemaMismatch { .. }
            | delta_bootstrap::StoreError::UnstampedOverlay
            | delta_bootstrap::StoreError::PreBaselineOverlay { .. }),
        ) => Some(store_err.to_string()),
        missing @ delta_bootstrap::Error::MissingCommand { .. } => Some(missing.to_string()),
        _ => None,
    }
}

/// Start the background loops `state` needs and serve the [`router`] on
/// `listener` until the server stops.
///
/// The listener's port must be the one the configuration `state` was built from
/// names, because the hook URLs rendered into each session's settings carry it.
pub async fn serve(state: AppState, listener: TcpListener) -> anyhow::Result<()> {
    // Continuously tail the transcript so assistant replies that Claude Code
    // flushes after the `Stop` hook still reach the browser within ~0.5s.
    state.spawn_transcript_tail();

    // Drain the interactor's async event seam into the broadcast, so a producer
    // that emits after its driving call returned still reaches browsers. This is
    // the only consumer of that seam, so without it those events go nowhere; for
    // who emits on it, see `Interactor::emit_async_event`.
    state.spawn_async_event_drain();

    let app = router(state);
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "delta-server listening (loopback only)");

    axum::serve(listener, app).await?;
    Ok(())
}
