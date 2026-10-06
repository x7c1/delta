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
//! 1. open the window at once, on a static placeholder page the shell serves
//!    itself ([`placeholder`]), so a launch shows a window within a moment
//!    however long the steps below take. Everything after this runs on a
//!    background thread, while the main thread runs the event loop;
//! 2. read the login shell's `PATH` and locale ([`login_env`]), so `tmux`,
//!    `claude` and `codex` are found and run in a UTF-8 locale when launched
//!    from Finder or a desktop file. They become the configuration's
//!    `child_env`, which the server sets on every command it starts, and the
//!    link opener gets them too;
//! 3. build the server configuration the CLI builds, under the app's bundle
//!    identifier: the server derives its data directory
//!    (`<platform data dir>/<identifier>`, the directory Tauri names the app
//!    data directory) and its tmux socket from it and creates the directory,
//!    so the dev build (`io.github.x7c1.delta.dev`) keeps apart from the
//!    installed app with no further settings;
//! 4. settle the hook secret and the port against the hook state file in the
//!    data directory: reuse the secret recorded there, and bind the port the
//!    previous launch recorded, falling back to a free `127.0.0.1:0` port when
//!    it is taken (an explicit `DELTA_PORT` wins and is not recorded). Both are
//!    written into the configuration before the state is built, since the hook
//!    URLs rendered into each session's settings carry them, and keeping them
//!    stable is what lets a session that survived a restart still reach Delta;
//! 5. build the state and serve on a tokio runtime the shell owns, then record
//!    the port where the window's link handlers read it ([`started_server`])
//!    and navigate the window from the placeholder to the server, on the main
//!    thread. The startup failures the user has to act on are shown in a
//!    message dialog over the placeholder, any other one in a generic dialog
//!    pointing to the log; a failed start exits 1 once it is dismissed.
//!
//! Steps 1 and 5 are each logged at `info`, so the time a launch spent behind
//! the placeholder can be read from the log.
//!
//! Links the page follows never take the window away from Delta; [`links`]
//! says where they go instead.
//!
//! The window opens at the size it was left at, maximized if it was; the first
//! time, at a size taken from the screen ([`window_size`]).
//!
//! Only one copy runs at a time: launching the app again focuses the running
//! window and exits (see [`focus_running_window`]).
//!
//! Closing the window ends the process and the server with it. The tmux server
//! on Delta's socket is left running, so the Claude Code sessions open in it
//! keep running, and the next launch re-adopts them before it serves anything.
//! Quitting from the keyboard goes the same way: Cmd-Q from macOS's default
//! application menu, and Ctrl-Q on Linux, which has no menu and gets the
//! shortcut from the shell (`quit_shortcut.rs`) — except while the terminal has
//! focus, where Ctrl-Q goes on to the pane. No menu is installed: replacing
//! macOS's default one would lose its Edit menu and Cmd-C/V/X/A in the page.
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
mod placeholder;
#[cfg(any(target_os = "linux", test))]
mod quit_shortcut;
mod started_server;
mod window_size;

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

use tauri::webview::NewWindowResponse;
use tauri::{App, AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_window_state::StateFlags;
use tokio::runtime::{self, Runtime};

use delta_server::{config, serve, AppState};
use started_server::StartedServer;

/// The window's label and title.
const WINDOW_LABEL: &str = "main";
const WINDOW_TITLE: &str = "Delta";

fn main() {
    serve::init_tracing();

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
        // Remembers the window's size and whether it is maximized; see
        // `window_size`. After single-instance, so a second launch exits before
        // this loads anything, and the running window is only brought forward.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::SIZE | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .manage(erase::EraseExit::default())
        .register_uri_scheme_protocol(placeholder::SCHEME, |_context, request| {
            placeholder::respond(&request)
        })
        // Tauri panics on an error returned from here, so every failure is
        // reported (and exits 1) inside the hook instead.
        .setup(move |app| {
            let started_server = Arc::new(StartedServer::default());
            let runtime_handle = runtime.handle().clone();
            // The runtime serves for the app's whole lifetime.
            app.manage(runtime);
            if let Err(err) = open_window(app, Arc::clone(&started_server)) {
                report_startup_failure(app.handle(), &err, None);
                return Ok(());
            }
            tracing::info!(
                "delta-desktop window shown on the placeholder page; starting the server"
            );
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                start_in_background(&handle, &runtime_handle, &identifier, &started_server)
            });
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
        // Plugins see `Exit` before this callback, so the window-state
        // plugin's file is already written when the erase removes the config
        // directory it lives in.
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
        // Not expected in practice: `setup` builds the window before anything
        // else, and a start that could not build it shows a dialog and exits.
        // The erase destroys the window, then also shows a dialog and exits.
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

/// Read the login shell's environment and start the server, off the main
/// thread; then show the server in the window, or report why it did not start,
/// on the main thread.
fn start_in_background(
    handle: &AppHandle,
    runtime: &runtime::Handle,
    identifier: &str,
    started_server: &StartedServer,
) {
    let login_env::LoginEnv {
        vars,
        path_not_imported,
    } = login_env::read_login_env();
    // A panic would otherwise end only this thread and leave the window on the
    // placeholder for good; the panic itself is already logged by the hook.
    let started = panic::catch_unwind(AssertUnwindSafe(|| {
        start_server(handle, runtime, identifier, vars.clone())
    }))
    .unwrap_or_else(|_| Err(anyhow::anyhow!("the server's start panicked")));
    let main_handle = handle.clone();
    let on_main_thread = match started {
        Ok(port) => {
            started_server.record(port, vars);
            handle.run_on_main_thread(move || show_server(&main_handle, port))
        }
        Err(err) => handle.run_on_main_thread(move || {
            report_startup_failure(&main_handle, &err, path_not_imported.as_ref())
        }),
    };
    if let Err(err) = on_main_thread {
        // The event loop is gone: the app is already quitting.
        tracing::warn!("could not hand the end of the start to the main thread: {err}");
    }
}

/// Navigate the window from the placeholder to the server on `port`. A failure
/// is reported like any other failed start.
fn show_server(handle: &AppHandle, port: u16) {
    let Some(window) = handle.get_webview_window(WINDOW_LABEL) else {
        tracing::info!(port, "the window was closed before the server was up");
        return;
    };
    let navigated = server_url(port).and_then(|url| Ok(window.navigate(url)?));
    match navigated {
        Ok(()) => tracing::info!(port, "delta-desktop window navigating to the server"),
        Err(err) => report_startup_failure(
            handle,
            &err.context("could not show the server in the window"),
            None,
        ),
    }
}

/// The server's root URL on the loopback `port`.
fn server_url(port: u16) -> anyhow::Result<Url> {
    Ok(format!("http://127.0.0.1:{port}/").parse()?)
}

/// Build the configuration and state, and start serving. Returns the port the
/// server listens on. `child_env` is set on every command the server starts.
fn start_server(
    handle: &AppHandle,
    runtime: &runtime::Handle,
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

    let handle = handle.clone();
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

/// Open the window on the placeholder page. Its link handlers read the
/// server's port and the link opener's environment from `started_server`,
/// which the background start fills in.
fn open_window(app: &App, started_server: Arc<StartedServer>) -> anyhow::Result<()> {
    let url = placeholder::url()?;
    let navigation_server = Arc::clone(&started_server);
    let builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::CustomProtocol(url))
        .title(WINDOW_TITLE)
        .inner_size(
            window_size::FALLBACK_SIZE.width,
            window_size::FALLBACK_SIZE.height,
        )
        .min_inner_size(window_size::MIN_SIZE.width, window_size::MIN_SIZE.height)
        .on_navigation(move |url| load_in_window(url, &navigation_server))
        .on_new_window(move |url, _features| {
            open_in_browser(
                &url,
                links::classify(&url, started_server.port()),
                started_server.opener_env(),
            );
            NewWindowResponse::Deny
        });
    #[cfg(target_os = "macos")]
    let builder = macos_title_bar::style(builder);
    #[cfg(target_os = "linux")]
    let terminal_focus = quit_shortcut::TerminalFocus::default();
    #[cfg(target_os = "linux")]
    let builder = quit_shortcut::style(builder, terminal_focus.clone());
    let window = builder.build()?;
    window_size::fit_window_size(app.handle(), &window, WINDOW_LABEL);
    #[cfg(target_os = "macos")]
    macos_title_bar::install_drag_strip(&window)?;
    // Without the shortcut the app still quits by closing the window.
    #[cfg(target_os = "linux")]
    if let Err(err) = quit_shortcut::install(&window, terminal_focus) {
        tracing::warn!("could not install the Ctrl-Q shortcut: {err:#}");
    }
    Ok(())
}

/// Whether a navigation stays in the window; one that does not is opened in the
/// default browser or refused.
fn load_in_window(url: &Url, started_server: &StartedServer) -> bool {
    let port = started_server.port();
    if placeholder::is_return_after_start(url, port) {
        tracing::info!(url = %url, "refused to go back to the placeholder page");
        return false;
    }
    let kind = links::classify(url, port);
    if kind == links::LinkKind::OwnOrigin {
        return true;
    }
    open_in_browser(url, kind, started_server.opener_env());
    false
}

/// Open a web link in the default browser; refuse and log anything else,
/// including the placeholder page, which only the window can show.
fn open_in_browser(url: &Url, kind: links::LinkKind, opener_env: &[(String, String)]) {
    match kind {
        links::LinkKind::OwnOrigin if placeholder::is_placeholder(url) => {
            tracing::warn!(url = %url, "refused to open the placeholder page in the browser");
        }
        links::LinkKind::OwnOrigin | links::LinkKind::Web => {
            links::open_externally(url, opener_env)
        }
        links::LinkKind::NotWeb => {
            tracing::warn!(url = %url, "refused to open a link that is not a web link");
        }
    }
}

/// Show a user-facing startup error in a dialog
/// ([`show_startup_failure_dialog`]). Any other error is logged and, when the
/// window exists, a generic dialog points to the log so that "Starting Delta…"
/// does not vanish unexplained; without a window it exits 1 straight away. A
/// missing command is explained by the login shell's `PATH` not having been
/// read, when it was not.
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
            show_startup_failure_dialog(handle, message);
        }
        None => {
            tracing::error!("delta-desktop could not start: {err:#}");
            if handle.get_webview_window(WINDOW_LABEL).is_some() {
                show_startup_failure_dialog(
                    handle,
                    "Delta could not start. See the log for details.".to_owned(),
                );
            } else {
                handle.exit(1);
            }
        }
    }
}

/// Show a startup failure in an error dialog, over the window when there is
/// one, and exit 1 when it is dismissed.
fn show_startup_failure_dialog(handle: &AppHandle, message: String) {
    let mut dialog = handle
        .dialog()
        .message(message)
        .title("Delta could not start")
        .kind(MessageDialogKind::Error);
    if let Some(window) = handle.get_webview_window(WINDOW_LABEL) {
        dialog = dialog.parent(&window);
    }
    let exit_handle = handle.clone();
    dialog.show(move |_| exit_handle.exit(1));
}
