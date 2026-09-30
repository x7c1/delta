//! Delta desktop shell.
//!
//! A launcher, not a second frontend: it starts `delta-server` inside its own
//! process on a loopback port and opens one webview window on
//! `http://127.0.0.1:<port>/`, where the server hands out the built SPA
//! (`embed-web`). Everything the page does — REST, `/ws`, `/pty`, `/comms`, and
//! the Claude Code hook callbacks to `/hooks/*` — goes over plain HTTP to that
//! loopback server, exactly as in the browser. The page is not served over
//! `tauri://` and uses no Tauri IPC.
//!
//! Startup, in order:
//!
//! 1. adopt the login shell's `PATH` ([`login_path`]), so `tmux`, `claude` and
//!    `codex` are found when launched from Finder or a desktop file;
//! 2. build the server configuration the CLI builds, with the database and the
//!    per-spawn working directories moved into the app data directory
//!    ([`app_data`]);
//! 3. bind `127.0.0.1:0` for a free port (an explicit `DELTA_PORT` wins) and
//!    write it into the configuration before the state is built, since the hook
//!    URLs rendered into each session's settings carry it;
//! 4. build the state and serve on a tokio runtime the shell owns, then open the
//!    window. The two startup failures the user has to act on are shown in a
//!    message dialog; either way a failed start exits 1.
//!
//! Closing the window ends the process and the server with it. The tmux server
//! on Delta's socket is left running, so open sessions survive a restart and
//! can be resumed.

mod app_data;
mod login_path;
#[cfg(target_os = "macos")]
mod macos_title_bar;

use tauri::{App, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio::runtime::Runtime;

use delta_server::{config, serve, AppState};

/// The window's label and title.
const WINDOW_LABEL: &str = "main";
const WINDOW_TITLE: &str = "Delta";

fn main() {
    serve::init_tracing();
    login_path::import_login_shell_path();

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!("could not start the async runtime: {err}");
            std::process::exit(1);
        }
    };

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Tauri panics on an error returned from here, so every failure is
        // reported (and exits 1) inside the hook instead.
        .setup(move |app| {
            let started = start_server(app, &runtime).and_then(|port| open_window(app, port));
            if let Err(err) = started {
                report_startup_failure(app.handle(), &err);
            }
            // The runtime serves for the app's whole lifetime.
            app.manage(runtime);
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(err) = result {
        tracing::error!("delta-app failed: {err:#}");
        std::process::exit(1);
    }
}

/// Build the configuration and state, and start serving. Returns the port the
/// server listens on.
fn start_server(app: &App, runtime: &Runtime) -> anyhow::Result<u16> {
    let data_dir = app.path().app_data_dir()?;
    let app_data::Placement {
        mut config,
        dirs_to_create,
    } = app_data::place_in_app_data_dir(
        config::config_from_env(),
        &data_dir,
        app_data::ExplicitPaths::from_env(),
    );
    for dir in &dirs_to_create {
        std::fs::create_dir_all(dir)
            .map_err(|err| anyhow::anyhow!("could not create {}: {err}", dir.display()))?;
    }
    tracing::info!(
        database = %config.database_path,
        sessions = %config.session_workdir_base,
        "delta-app data locations"
    );

    let port = config::port_from_env().unwrap_or(0);
    let listener = runtime.block_on(serve::bind_loopback(port))?;
    config.port = listener.local_addr()?.port();

    delta_server::log_claude_version(&config.launch.claude_bin);
    let state = runtime.block_on(AppState::build(&config))?;

    let handle = app.handle().clone();
    runtime.spawn(async move {
        if let Err(err) = serve::serve(state, listener).await {
            tracing::error!("delta-server stopped: {err:#}");
            handle.exit(1);
        }
    });
    Ok(config.port)
}

fn open_window(app: &App, port: u16) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{port}/").parse()?;
    let builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
        .title(WINDOW_TITLE)
        .inner_size(1280.0, 800.0);
    #[cfg(target_os = "macos")]
    let builder = macos_title_bar::style(builder);
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    let window = builder.build()?;
    #[cfg(target_os = "macos")]
    macos_title_bar::install_drag_strip(&window)?;
    Ok(())
}

/// Show a user-facing startup error in a message dialog and exit 1 when it is
/// dismissed; log any other error and exit 1 straight away.
fn report_startup_failure(handle: &AppHandle, err: &anyhow::Error) {
    match serve::user_facing_startup_error(err) {
        Some(message) => {
            tracing::error!("delta-app: {message}");
            let exit_handle = handle.clone();
            handle
                .dialog()
                .message(message)
                .title("Delta could not start")
                .kind(MessageDialogKind::Error)
                .show(move |_| exit_handle.exit(1));
        }
        None => {
            tracing::error!("delta-app could not start: {err:#}");
            handle.exit(1);
        }
    }
}
