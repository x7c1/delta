//! Importing the login shell's `PATH`.
//!
//! A GUI app launched from Finder or a desktop file inherits the session
//! manager's minimal `PATH` (on macOS `/usr/bin:/bin:/usr/sbin:/sbin`), not the
//! one the user's shell builds, so `tmux`, `claude` and `codex` — typically
//! under Homebrew, `~/.local/bin` or a version manager — are not found. Before
//! anything else runs, the shell asks the user's login shell for its `PATH` and
//! adopts it.
//!
//! The shell runs as an interactive login shell (`-i -l`), so both the profile
//! files (`.zprofile`, `.bash_profile`, `path_helper` on macOS) and the rc files
//! (`.zshrc`, `.bashrc`, where installers often append to `PATH`) are read.
//! The value is printed between two markers, so whatever the rc files write to
//! stdout around it is ignored. A shell that does not answer within
//! [`TIMEOUT`], exits without printing the markers, or cannot be spawned leaves
//! the inherited `PATH` in place with a warning.

use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How long the login shell may take to print its `PATH`.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// The shell used when `$SHELL` is unset or empty.
const FALLBACK_SHELL: &str = "/bin/sh";

/// Surrounds the printed `PATH`, so output the rc files produce is skipped.
const MARKER: &str = "__DELTA_LOGIN_SHELL_PATH__";

/// Replace the process `PATH` with the login shell's, or keep the inherited one
/// (with a warning) when it cannot be read.
///
/// Call it first thing in `main`, before other threads start: it mutates the
/// process environment.
pub fn import_login_shell_path() {
    let shell = login_shell(std::env::var_os("SHELL"));
    match read_login_shell_path(&shell) {
        Ok(path) => {
            tracing::info!(shell = %shell.to_string_lossy(), "using the login shell's PATH");
            std::env::set_var("PATH", path);
        }
        Err(reason) => tracing::warn!(
            shell = %shell.to_string_lossy(),
            "could not read the login shell's PATH ({reason}); keeping the inherited one"
        ),
    }
}

/// The shell to ask: `$SHELL`, or [`FALLBACK_SHELL`] when it is unset or empty.
pub fn login_shell(shell: Option<OsString>) -> OsString {
    shell
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| OsString::from(FALLBACK_SHELL))
}

/// The arguments that make a shell print its `PATH` between the markers, as an
/// interactive login shell.
///
/// `printf` with a quoted `"$PATH"` behaves the same in `sh`, `bash`, `zsh` and
/// `fish` (which joins its `PATH` list with `:` when quoted).
fn login_shell_args() -> [String; 4] {
    [
        "-i".to_owned(),
        "-l".to_owned(),
        "-c".to_owned(),
        format!("printf '%s%s%s' {MARKER} \"$PATH\" {MARKER}"),
    ]
}

/// The `PATH` printed between the markers in `stdout`, or `None` when the
/// markers are missing or enclose nothing.
pub fn parse_printed_path(stdout: &str) -> Option<String> {
    let start = stdout.find(MARKER)? + MARKER.len();
    let rest = &stdout[start..];
    let end = rest.find(MARKER)?;
    let path = rest[..end].trim();
    (!path.is_empty()).then(|| path.to_owned())
}

fn read_login_shell_path(shell: &OsString) -> Result<String, String> {
    let mut child = Command::new(shell)
        .args(login_shell_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("spawn failed: {err}"))?;
    let mut stdout = child.stdout.take().ok_or("no stdout pipe")?;

    // Read on a helper thread so the wait can time out: a process the rc files
    // start in the background may hold the pipe open after the shell exits, so
    // neither the read nor the wait is guaranteed to return.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        let result = stdout.read_to_string(&mut output).map(|_| output);
        let _ = sender.send(result);
    });
    let received = receiver.recv_timeout(TIMEOUT);
    if received.is_err() {
        let _ = child.kill();
    }
    // Reap the shell off the main thread; it has exited or been killed.
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    match received {
        Ok(Ok(output)) => {
            parse_printed_path(&output).ok_or_else(|| "the shell did not print a PATH".to_owned())
        }
        Ok(Err(err)) => Err(format!("reading its output failed: {err}")),
        Err(_) => Err(format!("no answer within {}s", TIMEOUT.as_secs())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_between_the_markers_is_taken() {
        let stdout = format!("{MARKER}/opt/homebrew/bin:/usr/bin{MARKER}");
        assert_eq!(
            parse_printed_path(&stdout).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
    }

    #[test]
    fn output_around_the_markers_is_ignored() {
        let stdout = format!("Last login: today\nwelcome!\n{MARKER}/a:/b\n{MARKER}bye\n");
        assert_eq!(parse_printed_path(&stdout).as_deref(), Some("/a:/b"));
    }

    #[test]
    fn missing_or_unterminated_markers_yield_none() {
        assert_eq!(parse_printed_path("/usr/bin:/bin"), None);
        assert_eq!(parse_printed_path(&format!("{MARKER}/usr/bin")), None);
    }

    #[test]
    fn an_empty_path_yields_none() {
        assert_eq!(parse_printed_path(&format!("{MARKER} \n{MARKER}")), None);
    }

    #[test]
    fn the_shell_defaults_to_bin_sh() {
        assert_eq!(login_shell(None), "/bin/sh");
        assert_eq!(login_shell(Some(OsString::new())), "/bin/sh");
        assert_eq!(login_shell(Some("/bin/zsh".into())), "/bin/zsh");
    }

    /// End to end against `/bin/sh`: the command the shell is given prints the
    /// `PATH` it was started with between the markers.
    #[test]
    fn a_real_shell_prints_its_path() {
        let path = read_login_shell_path(&OsString::from("/bin/sh")).expect("sh answers");
        assert!(!path.is_empty());
    }
}
