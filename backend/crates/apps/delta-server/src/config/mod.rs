//! Server configuration built from the environment.
//!
//! Shared by the `delta-server` binary and the desktop shell (`delta-app`), so
//! both read the same `DELTA_*` variables with the same defaults. Every value
//! has a local-friendly default, so a bare run needs no setup.
//!
//! The server boots fine when no tmux session exists yet: the session is created
//! lazily on the first `POST /api/sessions`, so none of these need a live session
//! at startup.

use std::ffi::OsString;

use delta_bootstrap::Config;

mod home_paths;
use home_paths::{default_worktree_base, transcript_root};

mod launch;
use launch::launch_from_vars;

mod secrets;
use secrets::{auth_token, hook_secret};

#[cfg(test)]
mod tests;

/// The port the server listens on when `DELTA_PORT` does not name one.
pub const DEFAULT_PORT: u16 = 7878;

/// The SQLite overlay path used when `DELTA_DB_PATH` is unset, relative to the
/// working directory.
const DEFAULT_DATABASE_PATH: &str = "delta.db";

/// The per-spawn working-directory base used when `DELTA_SESSION_WORKDIR` is
/// unset, relative to the working directory.
const DEFAULT_SESSION_WORKDIR: &str = ".tmp/session";

/// Build configuration from the process environment.
pub fn config_from_env() -> Config {
    config_from_vars(|name| std::env::var_os(name))
}

/// Build configuration from the variables `var` returns.
///
/// `var` answers like [`std::env::var_os`]. A value that is not valid Unicode is
/// treated as unset, exactly as [`std::env::var`] would; only `HOME` is read as a
/// raw OS string.
pub fn config_from_vars(var: impl Fn(&str) -> Option<OsString>) -> Config {
    let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
    let home = var("HOME");
    Config {
        database_path: text("DELTA_DB_PATH").unwrap_or_else(|| DEFAULT_DATABASE_PATH.to_owned()),
        session_workdir_base: text("DELTA_SESSION_WORKDIR")
            .unwrap_or_else(|| DEFAULT_SESSION_WORKDIR.to_owned()),
        worktree_base: text("DELTA_WORKTREE_BASE")
            .unwrap_or_else(|| default_worktree_base(home.clone())),
        tmux_socket: text("DELTA_TMUX_SOCKET")
            .unwrap_or_else(|| delta_bootstrap::DEFAULT_TMUX_SOCKET.to_owned()),
        auth_token: auth_token(text("DELTA_AUTH_TOKEN")),
        hook_secret: hook_secret(text("DELTA_HOOK_SECRET")),
        transcript_root: transcript_root(
            text("DELTA_TRANSCRIPT_ROOT"),
            text("CLAUDE_CONFIG_DIR"),
            home,
        ),
        port: port_from_vars(&text).unwrap_or(DEFAULT_PORT),
        launch: launch_from_vars(&text),
    }
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
