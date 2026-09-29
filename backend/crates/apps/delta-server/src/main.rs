//! Delta server binary.
//!
//! A thin wrapper around [`delta_server`]: it builds configuration from the
//! environment, constructs the shared [`AppState`], and serves the router on
//! `127.0.0.1` only — Delta is a local tool and never listens on a public
//! interface. All testable logic lives in the library crate.

use delta_server::{config, serve, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    serve::init_tracing();

    let config = config::config_from_env();

    // Record which upstream `claude` binary this server is running against,
    // before any session activity, so the boot banner carries the version
    // string for post-hoc debugging. Pure observability: a missing or failing
    // binary warns and continues — see `docs/guides/compatibility.md`
    // (subdomain 3) and the `claude_version` module docs for the contract.
    delta_server::log_claude_version(&config.launch.claude_bin);

    // The two startup failures the user has to act on get a clear line and exit
    // 1 with no backtrace (see `user_facing_startup_error`); every other failure
    // keeps the default `anyhow` propagation.
    let state = match AppState::build(&config).await {
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
    serve::serve(state, listener).await
}
