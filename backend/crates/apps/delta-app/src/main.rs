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
//! 1. adopt the login shell's `PATH` and locale ([`login_env`]), so `tmux`,
//!    `claude` and `codex` are found and run in a UTF-8 locale when launched
//!    from Finder or a desktop file;
//! 2. build the server configuration the CLI builds, with the database and the
//!    per-spawn working directories moved into the app data directory
//!    ([`app_data`]);
//! 3. settle the hook secret and the port against the hook state file beside
//!    the database: reuse the secret recorded there, and bind the port the
//!    previous launch recorded, falling back to a free `127.0.0.1:0` port when
//!    it is taken (an explicit `DELTA_PORT` wins and is not recorded). Both are
//!    written into the configuration before the state is built, since the hook
//!    URLs rendered into each session's settings carry them, and keeping them
//!    stable is what lets a session that survived a restart still reach Delta;
//! 4. build the state and serve on a tokio runtime the shell owns, then open the
//!    window. The startup failures the user has to act on are shown in a
//!    message dialog; either way a failed start exits 1.
//!
//! Links the page follows never take the window away from Delta; [`links`]
//! says where they go instead.
//!
//! Closing the window ends the process and the server with it. The tmux server
//! on Delta's socket is left running, so open sessions survive a restart and
//! can be resumed.

mod app_data;
mod links;
mod login_env;
#[cfg(target_os = "macos")]
mod macos_title_bar;

use tauri::webview::NewWindowResponse;
use tauri::{App, AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio::runtime::Runtime;

use delta_server::{config, serve, AppState};

/// The window's label and title.
const WINDOW_LABEL: &str = "main";
const WINDOW_TITLE: &str = "Delta";

fn main() {
    serve::init_tracing();
    login_env::import_login_shell_env();

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

    let mut hook_state = config::adopt_persisted_hook_secret(&mut config)?;
    let listener = runtime.block_on(serve::bind_app_listener(
        &mut config,
        config::port_from_env(),
        &mut hook_state,
    ))?;
    tracing::info!(
        state_file = %hook_state.path().display(),
        hook_endpoint_changed = config.hook_endpoint_changed,
        "delta-app hook endpoint settled"
    );

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
        .inner_size(1280.0, 800.0)
        .on_navigation(move |url| load_in_window(url, port))
        .on_new_window(move |url, _features| {
            open_in_browser(&url, links::classify(&url, port));
            NewWindowResponse::Deny
        });
    #[cfg(target_os = "macos")]
    let builder = macos_title_bar::style(builder);
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    let window = builder.build()?;
    #[cfg(target_os = "macos")]
    macos_title_bar::install_drag_strip(&window)?;
    Ok(())
}

/// Whether a navigation stays in the window; one that does not is opened in the
/// default browser or refused.
fn load_in_window(url: &Url, port: u16) -> bool {
    let kind = links::classify(url, port);
    if kind == links::LinkKind::OwnOrigin {
        return true;
    }
    open_in_browser(url, kind);
    false
}

/// Open a web link in the default browser; refuse and log anything else.
fn open_in_browser(url: &Url, kind: links::LinkKind) {
    match kind {
        links::LinkKind::OwnOrigin | links::LinkKind::Web => links::open_externally(url),
        links::LinkKind::NotWeb => {
            tracing::warn!(url = %url, "refused to open a link that is not a web link");
        }
    }
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
