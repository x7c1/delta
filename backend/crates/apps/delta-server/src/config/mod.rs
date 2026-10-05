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

use delta_bootstrap::{Config, DataDirError, DEFAULT_IDENTIFIER};

pub mod hook_state;
use hook_state::{HookStateError, HookStateFile};

mod home_paths;
use home_paths::{default_data_dir, default_worktree_base, transcript_root};

mod launch;
use launch::launch_from_vars;

mod secrets;
use secrets::{auth_token, hook_secret};

#[cfg(test)]
mod tests;

/// The port the server listens on when `DELTA_PORT` does not name one.
pub const DEFAULT_PORT: u16 = 7878;

/// Build configuration from the process environment, and create its data
/// directory.
///
/// The directories are created here, before anything opens a file in them
/// (the hook state file, then the database), so neither shell has to.
pub fn config_from_env() -> Result<Config, DataDirError> {
    prepared(config_from_vars(|name| std::env::var_os(name)))
}

/// [`config_from_env`] for a shell that runs under its own `identifier`, which
/// wins over `DELTA_IDENTIFIER`.
///
/// The desktop app passes its bundle identifier, so the dev build
/// (`io.github.x7c1.delta.dev`) gets its own data directory and tmux socket
/// with no further settings. `DELTA_DATA_DIR` and `DELTA_TMUX_SOCKET` still
/// override what the identifier implies.
pub fn config_from_env_for(identifier: &str) -> Result<Config, DataDirError> {
    prepared(config_from_vars_for(
        |name| std::env::var_os(name),
        Some(identifier.to_owned()),
    ))
}

/// `config` once its data directory exists.
fn prepared(config: Config) -> Result<Config, DataDirError> {
    config.data_layout().create_dirs()?;
    Ok(config)
}

/// Build configuration from the variables `var` returns.
///
/// `var` answers like [`std::env::var_os`]. A value that is not valid Unicode is
/// treated as unset, exactly as [`std::env::var`] would; only `HOME` is read as a
/// raw OS string. Creates nothing on disk.
pub fn config_from_vars(var: impl Fn(&str) -> Option<OsString>) -> Config {
    config_from_vars_for(var, None)
}

fn config_from_vars_for(
    var: impl Fn(&str) -> Option<OsString>,
    identifier: Option<String>,
) -> Config {
    let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
    let non_empty = |name: &str| text(name).filter(|value| !value.is_empty());
    let home = var("HOME");
    let identifier = identifier
        .or_else(|| non_empty("DELTA_IDENTIFIER"))
        .unwrap_or_else(|| DEFAULT_IDENTIFIER.to_owned());
    Config {
        data_dir: non_empty("DELTA_DATA_DIR").unwrap_or_else(|| default_data_dir(&identifier)),
        worktree_base: text("DELTA_WORKTREE_BASE")
            .unwrap_or_else(|| default_worktree_base(home.clone())),
        tmux_socket: non_empty("DELTA_TMUX_SOCKET").unwrap_or_else(|| identifier.clone()),
        identifier,
        auth_token: auth_token(text("DELTA_AUTH_TOKEN")),
        hook_secret: hook_secret(text("DELTA_HOOK_SECRET")),
        transcript_root: transcript_root(
            text("DELTA_TRANSCRIPT_ROOT"),
            text("CLAUDE_CONFIG_DIR"),
            home,
        ),
        port: port_from_vars(&text).unwrap_or(DEFAULT_PORT),
        hook_endpoint_changed: false,
        launch: launch_from_vars(&text),
    }
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
    config.hook_secret = settled.secret;
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
