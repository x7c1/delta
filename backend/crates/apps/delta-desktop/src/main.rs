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
//! Only one copy runs at a time: launching the app again focuses the running
//! window and exits (see [`focus_running_window`]).
//!
//! Closing the window ends the process and the server with it. The tmux server
//! on Delta's socket is left running, so the Claude Code sessions open in it
//! keep running, and the next launch re-adopts them before it serves anything.

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

    let context = tauri::generate_context!();
    #[cfg(target_os = "linux")]
    set_window_app_id(&context.config().identifier);

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!("could not start the async runtime: {err}");
            std::process::exit(1);
        }
    };

    let result = tauri::Builder::default()
        // Registered first, so a second launch is caught before anything else
        // runs: the plugin hands its arguments to the running instance and
        // exits, and its `setup` below — which would start a second server on
        // the same database and tmux socket — never runs. See
        // `focus_running_window`.
        //
        // On Linux `enableGTKAppId` also makes the GTK application unique on
        // the session bus, but that does not get in the way: plugins are set
        // up while the app is built, before the event loop runs, so a second
        // launch exits here before GTK would forward an `activate` to the
        // running copy.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_running_window(app);
        }))
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
        .run(context);
    if let Err(err) = result {
        tracing::error!("delta-desktop failed: {err:#}");
        std::process::exit(1);
    }
}

/// Make the identifier the window's app ID, so the desktop tells this build's
/// window apart from another build's (the dev environment's app and the
/// installed one are both an executable named `delta-desktop`).
///
/// GTK 3 takes a Wayland window's app ID (and the X11 `WM_CLASS`) from the
/// program name, not from the GTK application ID that `enableGTKAppId` sets, so
/// the program name has to be set too, before GTK starts. GNOME matches this ID
/// to the desktop entry's `StartupWMClass`.
#[cfg(target_os = "linux")]
fn set_window_app_id(identifier: &str) {
    glib::set_prgname(Some(identifier));
}

/// Bring the running instance's window forward when the app is launched again.
///
/// The desktop app is single-instance. A second copy would start a second
/// server on the same database and tmux socket; worse, the port the first copy
/// holds is the one recorded in the hook state file, so the second would fall
/// back to a fresh port, record that one over it and report the hook endpoint
/// as changed — and the next launch would then find every session the first
/// copy left running unable to reach it. A second launch therefore only focuses
/// the window that is already open. The CLI server is not affected.
fn focus_running_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        // The running instance has no window yet (it is still starting, or its
        // start failed and a dialog is up); there is nothing to focus.
        tracing::info!(
            "delta-desktop was launched again; the running instance has no window to focus"
        );
        return;
    };
    for (step, result) in [
        ("unminimize", window.unminimize()),
        ("show", window.show()),
        ("focus", window.set_focus()),
    ] {
        if let Err(err) = result {
            tracing::warn!("could not {step} the running window on a second launch: {err}");
        }
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
        "delta-desktop data locations"
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
        "delta-desktop hook endpoint settled"
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
            tracing::error!("delta-desktop: {message}");
            let exit_handle = handle.clone();
            handle
                .dialog()
                .message(message)
                .title("Delta could not start")
                .kind(MessageDialogKind::Error)
                .show(move |_| exit_handle.exit(1));
        }
        None => {
            tracing::error!("delta-desktop could not start: {err:#}");
            handle.exit(1);
        }
    }
}
