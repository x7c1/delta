//! Server configuration built from the environment.
//!
//! Shared by the `delta-server` binary and the desktop shell (`delta-desktop`), so
//! both read the same `DELTA_*` variables with the same defaults. Every value
//! has a local-friendly default, so a bare run needs no setup.
//!
//! Where the server writes is decided by two values: the **identifier**
//! (`DELTA_IDENTIFIER`, default [`DEFAULT_IDENTIFIER`]) and the **data
//! directory** (`DELTA_DATA_DIR`, default `<platform data dir>/<identifier>`).
//! Every file the server writes is derived from the data directory by
//! [`delta_bootstrap::DataLayout`]; the tmux socket defaults to the identifier.
//! A shell therefore only chooses the identifier — the desktop app passes its
//! bundle identifier ([`config_from_env_for`]), `make dev` sets
//! `DELTA_IDENTIFIER` — and the test harnesses point `DELTA_DATA_DIR` at their
//! run directory.
//!
//! The server boots fine when no tmux session exists yet: the session is created
//! lazily on the first `POST /api/sessions`, so none of these need a live session
//! at startup.
//!
//! One value is not from the environment alone: the hook secret is kept in a
//! state file in the data directory so it survives restarts, which
//! [`adopt_persisted_hook_secret`] applies.

use std::ffi::OsString;

use delta_bootstrap::{
    BuildOrigin, Config, DataDirError, DataDirOrigin, DataLayout, Launcher, DEFAULT_IDENTIFIER,
    DEFAULT_RELEASE_FEED_URL,
};

pub mod hook_state;
use hook_state::{HookStateError, HookStateFile};

mod home_paths;
use home_paths::{default_data_dir, default_worktree_base, transcript_root};

mod launch;
use launch::launch_from_vars;

mod secrets;
pub(crate) use secrets::mint_secret;
use secrets::{auth_token, hook_secret, AUTH_TOKEN_VAR};

#[cfg(test)]
mod tests;

/// The port the server listens on when `DELTA_PORT` does not name one.
pub const DEFAULT_PORT: u16 = 7878;

/// Where this build was made: `DELTA_BUILD_ORIGIN` as the compiler saw it,
/// judged by [`BuildOrigin::from_build_env`] (`release` only when the release
/// workflow's bundle step sets it; unset, empty or anything else is `local`).
/// Read at compile time, never at run time, so a running build cannot be
/// made to claim `release`; `build.rs` rebuilds this crate when the variable
/// changes.
pub fn build_origin() -> BuildOrigin {
    BuildOrigin::from_build_env(option_env!("DELTA_BUILD_ORIGIN"))
}

/// Build configuration from the process environment, and create its data
/// directory.
///
/// The directories are created here, before anything opens a file in them
/// (the hook state file, then the database), so neither shell has to.
///
/// The configuration of the CLI server: [`Config::launcher`] is
/// [`Launcher::Cli`].
///
/// Takes `DELTA_AUTH_TOKEN` out of the process environment once it is read
/// (`withhold_auth_token_from_children`).
pub fn config_from_env() -> Result<Config, DataDirError> {
    let config = config_from_vars(|name| std::env::var_os(name));
    withhold_auth_token_from_children();
    prepared(config)
}

/// [`config_from_env`] for a shell that runs under its own `identifier`, which
/// wins over `DELTA_IDENTIFIER`.
///
/// The desktop app passes its bundle identifier, so the dev build
/// (`io.github.x7c1.delta.dev`) gets its own data directory and tmux socket
/// with no further settings. `DELTA_DATA_DIR` and `DELTA_TMUX_SOCKET` still
/// override what the identifier implies.
///
/// Only the desktop shell builds its configuration here, so this is the one
/// path whose [`Config::launcher`] is [`Launcher::Desktop`].
pub fn config_from_env_for(identifier: &str) -> Result<Config, DataDirError> {
    let config = desktop_config_from_vars(|name| std::env::var_os(name), identifier);
    withhold_auth_token_from_children();
    prepared(config)
}

/// Remove `DELTA_AUTH_TOKEN` from this process's environment.
///
/// Every command the server starts — tmux, and through it each agent, git, gh,
/// the editor opener — inherits the process environment. The token is held in
/// the configuration once it is read, so nothing the server starts needs it,
/// and none of them should be able to read it: an agent runs whatever its
/// session asks, and its environment is visible to anything it runs in turn.
/// Called by the two `config_from_env*` entry points, which each shell calls
/// once, before the server starts any command: the `delta-server` binary
/// before its runtime starts a thread, the desktop shell on its startup thread
/// while the window's event loop already runs.
fn withhold_auth_token_from_children() {
    std::env::remove_var(AUTH_TOKEN_VAR);
}

/// [`config_from_vars`] for the desktop shell running under `identifier`.
fn desktop_config_from_vars(var: impl Fn(&str) -> Option<OsString>, identifier: &str) -> Config {
    config_from_vars_for(var, Some(identifier.to_owned()), Launcher::Desktop)
}

/// The data directory [`config_from_env_for`] gives a shell running under
/// `identifier`: `DELTA_DATA_DIR` when set, `<platform data dir>/<identifier>`
/// otherwise. Creates nothing on disk.
///
/// For what a shell reads from the data directory before it builds the whole
/// configuration — the desktop app's log filter, read before logging starts.
pub fn data_dir_from_env_for(identifier: &str) -> String {
    data_dir_from_vars(|name| std::env::var_os(name), identifier)
}

fn data_dir_from_vars(var: impl Fn(&str) -> Option<OsString>, identifier: &str) -> String {
    var("DELTA_DATA_DIR")
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_data_dir(identifier))
}

/// Create the data directory [`data_dir_from_env_for`] names for a shell
/// running under `identifier`, as the configuration will
/// ([`DataLayout::create_dirs`]).
///
/// For the desktop shell, which calls it first thing, before logging and
/// Tauri write into the directory: otherwise they would create it with the
/// umask's default mode, and the server's own call would find it already there.
pub fn create_data_dir_for(identifier: &str) -> Result<(), DataDirError> {
    let data_dir = data_dir_from_env_for(identifier);
    DataLayout::new(&data_dir).create_dirs(data_dir_origin(&data_dir, identifier))
}

/// [`DataDirOrigin::Default`] when `data_dir` is the directory Delta names for
/// `identifier`, [`DataDirOrigin::Explicit`] when `DELTA_DATA_DIR` named
/// another one.
fn data_dir_origin(data_dir: &str, identifier: &str) -> DataDirOrigin {
    if data_dir == default_data_dir(identifier) {
        DataDirOrigin::Default
    } else {
        DataDirOrigin::Explicit
    }
}

/// `config` once its data directory exists.
fn prepared(config: Config) -> Result<Config, DataDirError> {
    config
        .data_layout()
        .create_dirs(data_dir_origin(&config.data_dir, &config.identifier))?;
    Ok(config)
}

/// Build configuration from the variables `var` returns.
///
/// `var` answers like [`std::env::var_os`]. A value that is not valid Unicode is
/// treated as unset, exactly as [`std::env::var`] would; only `HOME` is read as a
/// raw OS string. Creates nothing on disk. The configuration is the CLI
/// server's ([`Launcher::Cli`]).
pub fn config_from_vars(var: impl Fn(&str) -> Option<OsString>) -> Config {
    config_from_vars_for(var, None, Launcher::Cli)
}

fn config_from_vars_for(
    var: impl Fn(&str) -> Option<OsString>,
    identifier: Option<String>,
    launcher: Launcher,
) -> Config {
    let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
    let non_empty = |name: &str| text(name).filter(|value| !value.is_empty());
    let home = var("HOME");
    let identifier = identifier
        .or_else(|| non_empty("DELTA_IDENTIFIER"))
        .unwrap_or_else(|| DEFAULT_IDENTIFIER.to_owned());
    Config {
        data_dir: data_dir_from_vars(&var, &identifier),
        worktree_base: text("DELTA_WORKTREE_BASE")
            .unwrap_or_else(|| default_worktree_base(home.clone())),
        tmux_socket: non_empty("DELTA_TMUX_SOCKET").unwrap_or_else(|| identifier.clone()),
        identifier,
        auth_token: auth_token(text(AUTH_TOKEN_VAR)).into(),
        hook_secret: hook_secret(text("DELTA_HOOK_SECRET")).into(),
        transcript_root: transcript_root(
            text("DELTA_TRANSCRIPT_ROOT"),
            text("CLAUDE_CONFIG_DIR"),
            home,
        ),
        port: port_from_vars(&text).unwrap_or(DEFAULT_PORT),
        hook_endpoint_changed: false,
        launch: launch_from_vars(&text),
        child_env: Vec::new(),
        release_feed_url: release_feed_url(text(RELEASE_FEED_URL_VAR), build_origin()),
        launcher,
        build_origin: build_origin(),
    }
}

/// The variable that points the release check elsewhere, or turns it off.
const RELEASE_FEED_URL_VAR: &str = "DELTA_RELEASE_FEED_URL";

/// Where the release check asks: [`DEFAULT_RELEASE_FEED_URL`] unless
/// `DELTA_RELEASE_FEED_URL` (`value`) says otherwise. The empty string turns
/// the check off in every build.
///
/// Another URL is honoured only by a `local` build, and only when it is
/// `https://`. A `release` build ignores it: the feed decides which update
/// the app offers to download and install, so the installed app asks
/// GitHub, whatever its environment says. A plain-HTTP URL is refused in any
/// build, since anyone on the path could answer it. Either way the check
/// asks the default feed, with a `warn` saying why.
fn release_feed_url(value: Option<String>, origin: BuildOrigin) -> Option<String> {
    let default = || Some(DEFAULT_RELEASE_FEED_URL.to_owned());
    match value {
        None => default(),
        Some(url) if url.is_empty() => None,
        Some(url) if origin == BuildOrigin::Release => {
            tracing::warn!(
                url,
                "{RELEASE_FEED_URL_VAR} is ignored in a release build; \
                 the release check asks {DEFAULT_RELEASE_FEED_URL}"
            );
            default()
        }
        Some(url) if !url.starts_with("https://") => {
            tracing::warn!(
                url,
                "{RELEASE_FEED_URL_VAR} is not an https:// URL, so it is ignored; \
                 the release check asks {DEFAULT_RELEASE_FEED_URL}"
            );
            default()
        }
        Some(url) => Some(url),
    }
}

/// `~/.delta` when `worktree_base` is the default `~/.delta/worktrees`: the
/// directory erasing everything also removes once it is empty. `None` for a
/// base the user chose (`DELTA_WORKTREE_BASE`), whose parent is theirs.
pub(crate) fn default_worktree_base_parent(worktree_base: &str) -> Option<String> {
    default_base_parent(worktree_base, std::env::var_os("HOME"))
}

fn default_base_parent(worktree_base: &str, home: Option<OsString>) -> Option<String> {
    let default = default_worktree_base(home);
    if worktree_base != default {
        return None;
    }
    std::path::Path::new(&default)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
}

/// Give `config` the hook secret kept in its data directory, and return the
/// state file it came from.
///
/// `DELTA_HOOK_SECRET` still wins when set; otherwise the recorded secret is
/// reused, or minted and recorded on a first run — see [`hook_state`]. Sets
/// [`Config::hook_endpoint_changed`] when the secret differs from the recorded
/// one. The desktop app goes on to settle its port against the same file
/// ([`crate::serve::bind_app_listener`]).
pub fn adopt_persisted_hook_secret(config: &mut Config) -> Result<HookStateFile, HookStateError> {
    adopt_hook_secret(config, std::env::var("DELTA_HOOK_SECRET").ok())
}

fn adopt_hook_secret(
    config: &mut Config,
    explicit: Option<String>,
) -> Result<HookStateFile, HookStateError> {
    let mut state = HookStateFile::open(config.data_layout().hook_state())?;
    let settled = state.settle_hook_secret(explicit)?;
    config.hook_secret = settled.secret.into();
    config.hook_endpoint_changed |= settled.changed;
    Ok(state)
}

/// The port `DELTA_PORT` names in the process environment, if it holds a valid
/// one. An unset or unparseable value is `None` (the CLI then uses
/// [`DEFAULT_PORT`]; the desktop shell takes a free port).
pub fn port_from_env() -> Option<u16> {
    port_from_vars(&|name: &str| std::env::var(name).ok())
}

fn port_from_vars(text: &dyn Fn(&str) -> Option<String>) -> Option<u16> {
    text("DELTA_PORT").and_then(|v| v.parse().ok())
}
