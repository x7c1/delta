//! Delta server binary.
//!
//! A thin wrapper around [`delta_server`]: it builds configuration from the
//! environment, constructs the shared [`AppState`], and serves the router on
//! `127.0.0.1` only — Delta is a local tool and never listens on a public
//! interface. All testable logic lives in the library crate.
//!
//! On start it prints, on one line of stderr, the URL that opens the app: the
//! server's origin with a one-time code in the fragment. It also keeps its
//! bearer token in the token file in its data directory, owner-only, for
//! `delta-server open-url` and other local clients, and removes it on a clean
//! stop: an erase from Settings → Storage (exit `0`), or `SIGINT` / `SIGTERM`
//! (exit `130` / `143`).
//!
//! `delta-server open-url [--origin <url>]` prints a fresh URL for the server
//! already running on the same data directory and port, without restarting it.

use delta_server::open_url::{self, OpenUrlError};
use delta_server::token_file::TokenFile;
use delta_server::{config, serve, AppState, CODE_LIFETIME};
use tokio::signal::unix::{signal, SignalKind};

const USAGE: &str = "\
usage: delta-server                        run the server
       delta-server open-url [--origin URL]
                                           print a URL with a fresh one-time code
                                           for the server already running on this
                                           data directory and port (DELTA_IDENTIFIER,
                                           DELTA_DATA_DIR, DELTA_PORT); --origin
                                           prints it for another origin, such as
                                           the Vite dev server";

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => run_server(),
        Some("open-url") => run_open_url(args),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => {
            eprintln!("delta-server: unexpected argument: {other}\n{USAGE}");
            std::process::exit(2);
        }
    }
}

/// `delta-server open-url`: print the URL to stdout, or say why there is none
/// and exit 1 (2 for a usage error).
fn run_open_url(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let result = open_url::parse_args(args).and_then(|args| {
        // Read the same variables the server read, without creating anything.
        let config = config::config_from_vars(|name| std::env::var_os(name));
        open_url::open_url(&args, config.data_layout().auth_token(), config.port)
    });
    match result {
        Ok(url) => {
            println!("{url}");
            Ok(())
        }
        Err(err @ OpenUrlError::Usage(_)) => {
            eprintln!("delta-server open-url: {err}\n{USAGE}");
            std::process::exit(2);
        }
        Err(err) => {
            eprintln!("delta-server open-url: {err}");
            std::process::exit(1);
        }
    }
}

fn run_server() -> anyhow::Result<()> {
    serve::init_tracing();

    // Read before the runtime starts any thread: this also takes
    // `DELTA_AUTH_TOKEN` out of the environment the server's children inherit.
    let config = config::config_from_env()?;
    tokio::runtime::Runtime::new()?.block_on(serve_until_stopped(config))
}

async fn serve_until_stopped(mut config: delta_bootstrap::Config) -> anyhow::Result<()> {
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
    delta_server::log_claude_version(&config.launch.claude_bin, &config.child_env);

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

    // Written only once the port is ours: a second server started on the same
    // data directory fails to bind here, and must not replace (then remove) the
    // token file of the server that holds the port. Written right after the
    // bind, before anything is served, so a client that waits for the port
    // finds it.
    let listener = serve::bind_loopback(config.port).await?;
    let token_file = write_token_file(&config, &state);
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    print_open_url(&state, config.port);

    // The server serves until something asks it to stop; every reason it
    // can stop for is a clean exit, and so is a signal.
    let exit_code = tokio::select! {
        stopped = serve::serve(state, listener) => {
            stopped?.log();
            0
        }
        _ = interrupt.recv() => 130,
        _ = terminate.recv() => 143,
    };
    drop(token_file);
    if exit_code != 0 {
        tracing::info!(exit_code, "delta-server stopped on a signal");
        // Exit here rather than return: dropping the runtime would wait on its
        // blocking tasks, such as a terminal bridge reading its pane.
        std::process::exit(exit_code);
    }
    Ok(())
}

/// Keep the bearer token in the data directory for local clients. A failure is
/// logged, not fatal: the server works without it, only `open-url` does not.
fn write_token_file(config: &delta_bootstrap::Config, state: &AppState) -> Option<TokenFile> {
    let path = config.data_layout().auth_token();
    match TokenFile::write(&path, state.token()) {
        Ok(file) => Some(file),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                "could not write the bearer token file; `delta-server open-url` will not \
                 find this server: {err}"
            );
            None
        }
    }
}

/// Print the URL that opens the app, with a fresh one-time code, on one line.
///
/// Only a build with the web app compiled in (`embed-web`) serves the app at
/// its own origin; without it the app is served elsewhere (the Vite dev
/// server), so the line says how to get a URL for that origin instead.
fn print_open_url(state: &AppState, port: u16) {
    if cfg!(feature = "embed-web") {
        let url = open_url::code_url(
            &open_url::loopback_origin(port),
            &state.mint_one_time_code(),
        );
        eprintln!(
            "delta-server: open Delta at {url} (the link works once, within {} minutes; \
             `delta-server open-url` prints a new one)",
            CODE_LIFETIME.as_secs() / 60
        );
    } else {
        eprintln!(
            "delta-server: this build serves no web app (built without `embed-web`); \
             `delta-server open-url --origin <web app origin>` prints a link that opens it"
        );
    }
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
