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
//! 1. read the login shell's `PATH` and locale ([`login_env`]), so `tmux`,
//!    `claude` and `codex` are found and run in a UTF-8 locale when launched
//!    from Finder or a desktop file. They become the configuration's
//!    `child_env`, which the server sets on every command it starts, and the
//!    link opener gets them too;
//! 2. build the server configuration the CLI builds, under the app's bundle
//!    identifier: the server derives its data directory
//!    (`<platform data dir>/<identifier>`, the directory Tauri names the app
//!    data directory) and its tmux socket from it and creates the directory,
//!    so the dev build (`io.github.x7c1.delta.dev`) keeps apart from the
//!    installed app with no further settings;
//! 3. settle the hook secret and the port against the hook state file in the
//!    data directory: reuse the secret recorded there, and bind the port the
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
//!
//! Erasing everything from Settings → Storage ends it the other way round: the
//! server stops on its own and returns what it kept ([`serve::ServerStopped`]).
//! The shell then closes the window, shows what was kept in a message dialog,
//! and quits when it is dismissed; as the process ends ([`RunEvent::Exit`]) it
//! removes its own files under the identifier ([`erase`]).

mod erase;
mod links;
mod login_env;
#[cfg(target_os = "macos")]
mod macos_title_bar;

use std::sync::Arc;

use tauri::webview::NewWindowResponse;
use tauri::{App, AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio::runtime::Runtime;

use delta_server::{config, serve, AppState};

/// The window's label and title.
const WINDOW_LABEL: &str = "main";
const WINDOW_TITLE: &str = "Delta";

fn main() {
    serve::init_tracing();
    let login_env::LoginEnv {
        vars: child_env,
        path_not_imported,
    } = login_env::read_login_env();

    let context = tauri::generate_context!();
    let identifier = context.config().identifier.clone();
    #[cfg(target_os = "linux")]
    set_window_app_id(&identifier);

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!("could not start the async runtime: {err}");
            std::process::exit(1);
        }
    };

    let app = tauri::Builder::default()
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
        .manage(erase::EraseExit::default())
        // Tauri panics on an error returned from here, so every failure is
        // reported (and exits 1) inside the hook instead.
        .setup(move |app| {
            let started = start_server(app, &runtime, &identifier, child_env.clone())
                .and_then(|port| open_window(app, port, child_env));
            if let Err(err) = started {
                report_startup_failure(app.handle(), &err, path_not_imported.as_ref());
            }
            // The runtime serves for the app's whole lifetime.
            app.manage(runtime);
            Ok(())
        })
        .build(context);
    let app = match app {
        Ok(app) => app,
        Err(err) => {
            tracing::error!("delta-desktop failed: {err:#}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| match event {
        // The erase closes the window before its dialog is up; closing the
        // last window must not end the app before the dialog is dismissed
        // (`code` is `None` for an exit the window's closing asked for).
        RunEvent::ExitRequested {
            code: None, api, ..
        } if handle.state::<erase::EraseExit>().is_marked() => api.prevent_exit(),
        RunEvent::Exit if handle.state::<erase::EraseExit>().is_marked() => {
            erase::remove_app_dirs(handle);
        }
        _ => {}
    });
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
/// server listens on. `child_env` is set on every command the server starts.
fn start_server(
    app: &App,
    runtime: &Runtime,
    identifier: &str,
    child_env: Vec<(String, String)>,
) -> anyhow::Result<u16> {
    let mut config = config::config_from_env_for(identifier)?;
    config.child_env = child_env;
    tracing::info!(
        identifier = %config.identifier,
        data_dir = %config.data_dir,
        tmux_socket = %config.tmux_socket,
        "delta-desktop data directory"
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

    delta_server::log_claude_version(&config.launch.claude_bin, &config.child_env);
    let state = runtime.block_on(AppState::build(&config))?;

    let handle = app.handle().clone();
    runtime.spawn(async move {
        match serve::serve(state, listener).await {
            Ok(stopped) => {
                stopped.log();
                match stopped {
                    // Erased from Settings → Storage: the server has deleted
                    // its data directory, and the app has nothing left to show.
                    serve::ServerStopped::Erased(report) => {
                        quit_after_erase(&handle, erase::report_message(report.kept_items()));
                    }
                }
            }
            Err(err) => {
                tracing::error!("delta-server stopped: {err:#}");
                handle.exit(1);
            }
        }
    });
    Ok(config.port)
}

/// Close the window, show what the erase kept, and exit 0 once the dialog is
/// dismissed. The exit is marked as an erase first, which `main`'s run loop
/// acts on.
fn quit_after_erase(handle: &AppHandle, message: String) {
    handle.state::<erase::EraseExit>().mark();
    if let Some(window) = handle.get_webview_window(WINDOW_LABEL) {
        // Destroyed rather than closed: the webview must be gone before the
        // process ends, and nothing may veto it.
        if let Err(err) = window.destroy() {
            tracing::warn!("could not close the window after the erase: {err}");
        }
    }
    let exit_handle = handle.clone();
    handle
        .dialog()
        .message(message)
        .title("Delta erased everything")
        .kind(MessageDialogKind::Info)
        .show(move |_| exit_handle.exit(0));
}

/// Open the window on the server's port. Links handed to the default browser
/// are opened with `opener_env` set on the opener.
fn open_window(app: &App, port: u16, opener_env: Vec<(String, String)>) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{port}/").parse()?;
    let opener_env: Arc<[(String, String)]> = opener_env.into();
    let navigation_env = Arc::clone(&opener_env);
    let builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
        .title(WINDOW_TITLE)
        .inner_size(1280.0, 800.0)
        .on_navigation(move |url| load_in_window(url, port, &navigation_env))
        .on_new_window(move |url, _features| {
            open_in_browser(&url, links::classify(&url, port), &opener_env);
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
fn load_in_window(url: &Url, port: u16, opener_env: &[(String, String)]) -> bool {
    let kind = links::classify(url, port);
    if kind == links::LinkKind::OwnOrigin {
        return true;
    }
    open_in_browser(url, kind, opener_env);
    false
}

/// Open a web link in the default browser; refuse and log anything else.
fn open_in_browser(url: &Url, kind: links::LinkKind, opener_env: &[(String, String)]) {
    match kind {
        links::LinkKind::OwnOrigin | links::LinkKind::Web => {
            links::open_externally(url, opener_env)
        }
        links::LinkKind::NotWeb => {
            tracing::warn!(url = %url, "refused to open a link that is not a web link");
        }
    }
}

/// Show a user-facing startup error in a message dialog and exit 1 when it is
/// dismissed; log any other error and exit 1 straight away. A missing command
/// is explained by the login shell's `PATH` not having been read, when it was
/// not.
fn report_startup_failure(
    handle: &AppHandle,
    err: &anyhow::Error,
    path_not_imported: Option<&login_env::PathNotImported>,
) {
    match serve::user_facing_startup_error(err) {
        Some(message) => {
            let message = match path_not_imported {
                Some(not_imported) => not_imported.explain(message, err),
                None => message,
            };
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
