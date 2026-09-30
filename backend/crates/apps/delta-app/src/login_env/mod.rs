//! Importing the login shell's `PATH` and locale.
//!
//! A GUI app launched from Finder or a desktop file inherits the session
//! manager's minimal environment, not the one the user's shell builds:
//!
//! - its `PATH` (on macOS `/usr/bin:/bin:/usr/sbin:/sbin`) misses `tmux`,
//!   `claude` and `codex`, typically under Homebrew, `~/.local/bin` or a
//!   version manager;
//! - it has no `LANG` / `LC_*`, so `tmux`, Claude Code, git and the other
//!   commands the sessions run fall back to the ASCII-only `C` locale.
//!
//! Before anything else runs, the app asks the user's login shell for its
//! environment and adopts `PATH`, `LANG` and every `LC_*` from it. When no
//! locale that decides the character set (`LC_ALL`, `LC_CTYPE`, `LANG`) is set
//! afterwards, [`FALLBACK_LANG`](locale_fallback::FALLBACK_LANG) is set so
//! every command sees a UTF-8 locale.
//!
//! The shell runs as an interactive login shell (`-i -l`), so both the profile
//! files (`.zprofile`, `.bash_profile`, `path_helper` on macOS) and the rc files
//! (`.zshrc`, `.bashrc`, where installers often append to `PATH`) are read.
//! Its `env` output is printed between two markers, so whatever the rc files
//! write to stdout around it is ignored. A shell that does not answer within
//! [`TIMEOUT`](read_login_shell_env::TIMEOUT), exits without printing the
//! markers, or cannot be spawned leaves the inherited environment in place with
//! a warning; the locale fallback applies either way.

mod locale_fallback;
use locale_fallback::locale_fallback;

mod login_shell;
use login_shell::login_shell;

mod parse_printed_env;
use parse_printed_env::parse_printed_env;

mod read_login_shell_env;
use read_login_shell_env::read_login_shell_env;

/// Surrounds the printed environment, so output the rc files produce is
/// skipped.
const MARKER: &str = "__DELTA_LOGIN_SHELL_ENV__";

/// Adopt the login shell's `PATH` and locale, or keep the inherited ones (with
/// a warning) when they cannot be read, then fall back to a UTF-8 `LANG` when
/// no locale is set.
///
/// Call it first thing in `main`, before other threads start: it mutates the
/// process environment.
pub fn import_login_shell_env() {
    let shell = login_shell(std::env::var_os("SHELL"));
    match read_login_shell_env(&shell) {
        Ok(vars) => {
            if !vars.iter().any(|(name, _)| name == "PATH") {
                tracing::warn!(
                    shell = %shell.to_string_lossy(),
                    "the login shell printed no PATH; keeping the inherited one"
                );
            }
            let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
            tracing::info!(
                shell = %shell.to_string_lossy(),
                vars = ?names,
                "using the login shell's environment"
            );
            for (name, value) in &vars {
                std::env::set_var(name, value);
            }
        }
        Err(reason) => tracing::warn!(
            shell = %shell.to_string_lossy(),
            "could not read the login shell's environment ({reason}); keeping the inherited one"
        ),
    }
    if let Some((name, value)) = locale_fallback(|name| std::env::var_os(name)) {
        tracing::info!("no locale is set; using {name}={value}");
        std::env::set_var(name, value);
    }
}
