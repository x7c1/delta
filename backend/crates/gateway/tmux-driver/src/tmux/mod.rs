//! [`Tmux`]: the concrete [`TmuxDriver`](delta_usecase::TmuxDriver).
//!
//! Split by responsibility: this module holds the driver struct and the two
//! command-running helpers every call goes through, `conf` holds Delta's fixed
//! tmux configuration and its hardened write, `commands` builds the `tmux`
//! argv vectors (pure functions, unit-tested without a tmux server), and
//! `driver` wires the [`TmuxDriver`](delta_usecase::TmuxDriver) trait up on top
//! of them.

mod commands;
mod conf;
mod driver;

use tokio::process::Command;

use crate::error::Error;
use crate::TMUX_BIN;

/// Drives Claude Code sessions living in tmux.
///
/// The driver is stateless with respect to any particular session: every method
/// takes the target session name (or pane) explicitly, so one driver instance
/// manages any number of concurrent sessions. Session names are minted by the
/// caller (Delta's registry), never derived from Claude's `session_id`, so
/// resuming a conversation under a fresh name never collides with a live one.
///
/// Every command runs against Delta's **own tmux server** via a dedicated socket
/// (`tmux -L <socket>`), kept separate from the user's default tmux server. This
/// isolation means Delta's sessions never clutter the user's `tmux ls` and
/// teardown can kill the whole server at once. The server also starts with
/// Delta's fixed config (`-f`, see [`conf::DELTA_TMUX_CONF`]) instead of the
/// user's `~/.tmux.conf`, so the embedded pane is identical on every machine.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// The dedicated tmux socket name (`tmux -L <socket>`).
    socket: String,
    /// Path to the rendered [`conf::DELTA_TMUX_CONF`] file passed via `tmux -f`.
    ///
    /// Chosen by the caller (the server derives it from its data directory, so
    /// servers with different data directories never share a file). Written by
    /// [`create_session`](delta_usecase::TmuxDriver::create_session) before the
    /// server starts.
    conf_path: String,
}

impl Tmux {
    /// Create a driver bound to a dedicated tmux socket, starting that socket's
    /// server with the configuration written to `conf_path`.
    pub fn new(socket: impl Into<String>, conf_path: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            conf_path: conf_path.into(),
        }
    }

    /// Run `tmux -L <socket> -f <conf> <args>`, returning the captured output.
    ///
    /// The `-L <socket>` prefix pins every command to Delta's own tmux server.
    /// The `-f <conf>` prefix makes that server load Delta's fixed config instead
    /// of the user's `~/.tmux.conf`. `-f` is only consulted when the server
    /// starts (by `new-session`, see
    /// [`create_session`](delta_usecase::TmuxDriver::create_session)) and is
    /// harmlessly ignored on every other command, so passing it on all of them
    /// guarantees whichever call boots the server uses Delta's config.
    async fn output(&self, args: &[&str]) -> std::result::Result<std::process::Output, Error> {
        Ok(Command::new(TMUX_BIN)
            .arg("-L")
            .arg(&self.socket)
            .arg("-f")
            .arg(&self.conf_path)
            .args(args)
            .output()
            .await?)
    }

    /// Run `tmux <args>`, erroring on a non-zero exit.
    async fn run(&self, args: &[&str]) -> std::result::Result<(), Error> {
        self.captured(args).await.map(|_| ())
    }

    /// Run `tmux <args>` and return its stdout, erroring on a non-zero exit.
    ///
    /// The reading twin of [`Self::run`] — same command, same failure handling
    /// — for the one call whose output is the point (`capture-pane`).
    async fn captured(&self, args: &[&str]) -> std::result::Result<String, Error> {
        let output = self.output(args).await?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(Error::Command {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}

/// Where tmux puts the socket of the server named `socket` (`tmux -L`):
/// `${TMUX_TMPDIR:-/tmp}/tmux-<uid>/<socket>`.
///
/// tmux leaves this file behind when its server is killed, so stopping the
/// server for good means unlinking it here (checked on tmux 3.6a). An empty
/// `TMUX_TMPDIR` counts as unset, as it does for tmux.
fn socket_path(
    tmux_tmpdir: Option<std::ffi::OsString>,
    uid: u32,
    socket: &str,
) -> std::path::PathBuf {
    let tmpdir = tmux_tmpdir.filter(|dir| !dir.is_empty()).map_or_else(
        || std::path::PathBuf::from("/tmp"),
        std::path::PathBuf::from,
    );
    tmpdir.join(format!("tmux-{uid}")).join(socket)
}

/// Whether a failed tmux command's `stderr` says there is no server to talk
/// to: none was ever started on the socket (`error connecting to …`), or the
/// one there has gone (`no server running on …`).
fn is_no_server(stderr: &str) -> bool {
    stderr.starts_with("no server running") || stderr.starts_with("error connecting to")
}

#[cfg(test)]
mod tests {
    use delta_usecase::pane_for;

    use super::*;

    #[test]
    fn pane_for_derives_first_pane_of_session() {
        assert_eq!(pane_for("delta-1"), "delta-1:0.0");
    }

    #[test]
    fn the_socket_lives_under_tmux_tmpdir_or_tmp_per_user() {
        assert_eq!(
            socket_path(None, 501, "delta"),
            std::path::PathBuf::from("/tmp/tmux-501/delta")
        );
        assert_eq!(
            socket_path(Some("".into()), 501, "delta"),
            std::path::PathBuf::from("/tmp/tmux-501/delta")
        );
        assert_eq!(
            socket_path(Some("/run/user/501".into()), 501, "delta"),
            std::path::PathBuf::from("/run/user/501/tmux-501/delta")
        );
    }

    #[test]
    fn a_missing_or_gone_server_is_told_apart_from_a_failure() {
        assert!(is_no_server("no server running on /tmp/tmux-501/delta"));
        assert!(is_no_server(
            "error connecting to /tmp/tmux-501/delta (No such file or directory)"
        ));
        assert!(!is_no_server("unknown command: kill-servr"));
    }

    #[test]
    fn conf_path_is_the_one_the_caller_chose() {
        let tmux = Tmux::new("delta", "/data/delta/tmux.conf");
        assert_eq!(tmux.socket, "delta");
        assert_eq!(tmux.conf_path, "/data/delta/tmux.conf");
    }
}
