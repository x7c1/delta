//! Delta server binary.
//!
//! A thin wrapper around [`delta_server`]: it builds configuration from the
//! environment, constructs the shared [`AppState`], and serves the router on
//! `127.0.0.1` only — Delta is a local tool and never listens on a public
//! interface. It exits `0` once the server stops (an erase from Settings →
//! Storage stops it). All testable logic lives in the library crate.

use delta_server::{config, serve, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    serve::init_tracing();

    let mut config = config::config_from_env()?;
    tracing::info!(
        identifier = %config.identifier,
        data_dir = %config.data_dir,
        tmux_socket = %config.tmux_socket,
        "delta-server data directory"
    );

    // Record which upstream `claude` binary this server is running against,
    // before any session activity, so the boot banner carries the version
    // string for post-hoc debugging. Pure observability: a missing or failing
    // binary warns and continues — see `docs/guides/compatibility.md`
    // (subdomain 3) and the `claude_version` module docs for the contract.
    delta_server::log_claude_version(&config.launch.claude_bin);

    // The startup failures the user has to act on get a clear line and exit 1
    // with no backtrace (see `user_facing_startup_error`); every other failure
    // keeps the default `anyhow` propagation.
    let state = match build_state(&mut config).await {
        Ok(state) => state,
        Err(err) => match serve::user_facing_startup_error(&err) {
            Some(message) => {
                eprintln!("delta-server: {message}");
                std::process::exit(1);
            }
            None => return Err(err),
        },
    };

    let listener = serve::bind_loopback(config.port).await?;
    // The server serves until something asks it to stop; every reason it
    // can stop for is a clean exit.
    serve::serve(state, listener).await?.log();
    Ok(())
}

/// Settle the hook secret kept in the data directory, then build the state.
///
/// The port stays the fixed one the configuration names (`DELTA_PORT`, else
/// [`config::DEFAULT_PORT`]); only the desktop app records and re-chooses its
/// port.
async fn build_state(config: &mut delta_bootstrap::Config) -> anyhow::Result<AppState> {
    config::adopt_persisted_hook_secret(config)?;
    AppState::build(config).await
}
