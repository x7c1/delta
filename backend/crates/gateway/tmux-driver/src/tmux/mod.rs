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
    /// Per-socket so concurrent Delta servers on different sockets never share a
    /// file. Written by
    /// [`create_session`](delta_usecase::TmuxDriver::create_session) before the
    /// server starts.
    conf_path: String,
}

impl Tmux {
    /// Create a driver bound to a dedicated tmux socket.
    pub fn new(socket: impl Into<String>) -> Self {
        let socket = socket.into();
        let conf_path = std::env::temp_dir()
            .join(format!("delta-tmux-{socket}.conf"))
            .to_string_lossy()
            .into_owned();
        Self { socket, conf_path }
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
        Ok(Command::new("tmux")
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
        let output = self.output(args).await?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::Command {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
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
    fn conf_path_is_derived_per_socket() {
        // The config path is namespaced by socket so concurrent Delta servers on
        // different sockets never write over each other's config.
        assert!(Tmux::new("delta")
            .conf_path
            .ends_with("delta-tmux-delta.conf"));
        assert!(Tmux::new("other")
            .conf_path
            .ends_with("delta-tmux-other.conf"));
    }
}
