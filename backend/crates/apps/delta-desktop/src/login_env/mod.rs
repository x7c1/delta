//! Reading the login shell's `PATH` and locale.
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
//! At startup, before the server starts, the app asks the user's login shell
//! for its environment and keeps `PATH`, `LANG` and every `LC_*` from it as a
//! [`LoginEnv`], so the sessions the server re-adopts or launches always get
//! these values. When no
//! locale that decides the character set (`LC_ALL`, `LC_CTYPE`, `LANG`) is set
//! by the shell or inherited, [`FALLBACK_LANG`](locale_fallback::FALLBACK_LANG)
//! is added so every command sees a UTF-8 locale. The process environment is
//! never written: the pairs go into the server configuration
//! (`Config::child_env`).
//!
//! The shell runs as an interactive login shell (`-i -l`), so both the profile
//! files (`.zprofile`, `.bash_profile`, `path_helper` on macOS) and the rc files
//! (`.zshrc`, `.bashrc`, where installers often append to `PATH`) are read.
//! Its `env` output is printed between two markers, so whatever the rc files
//! write to stdout around it is ignored. A shell that does not answer within
//! [`TIMEOUT`](read_login_shell_env::TIMEOUT), exits without printing the
//! markers, or cannot be spawned yields no pairs from the shell, so the
//! commands keep the inherited environment, with a warning. The timeout bounds
//! how long the server's start waits for the shell. The locale fallback
//! applies either way. Why the `PATH` was not read is returned as a
//! [`PathNotImported`], so a startup that then stops on a missing command can
//! say so in its dialog.

mod locale_fallback;
use locale_fallback::locale_fallback;

mod login_shell;
use login_shell::login_shell;

mod parse_printed_env;
use parse_printed_env::parse_printed_env;

mod path_not_imported;
pub use path_not_imported::PathNotImported;

mod read_login_env;
pub use read_login_env::read_login_env;

mod read_login_shell_env;
use read_login_shell_env::read_login_shell_env;

/// Surrounds the printed environment, so output the rc files produce is
/// skipped.
const MARKER: &str = "__DELTA_LOGIN_SHELL_ENV__";

/// What the app read from the login shell: the variables every command
/// Delta starts is given, and why the `PATH` among them is missing, if it is.
#[derive(Debug)]
pub struct LoginEnv {
    /// `PATH`, `LANG` and `LC_*` as the login shell printed them, in the order
    /// printed, followed by the UTF-8 `LANG` fallback when none of them (nor the
    /// inherited environment) sets a locale that decides the character set.
    pub vars: Vec<(String, String)>,
    /// Why the login shell's `PATH` is not among [`Self::vars`], if it is not.
    pub path_not_imported: Option<PathNotImported>,
}
