//! Driving Claude Code sessions by sending keystrokes to tmux panes.

use async_trait::async_trait;

use crate::error::Result;

/// Drives Claude Code sessions: manages tmux session lifecycles and sends
/// keystrokes to their panes.
///
/// The trait is stateless with respect to any particular session: every method
/// takes the target session name (or pane) explicitly, so a single driver can
/// manage any number of concurrent sessions. The session name is a Delta-minted
/// identifier (never Claude's `session_id`), so resuming a conversation under a
/// fresh name never collides with an existing one.
#[async_trait]
pub trait TmuxDriver: Send + Sync {
    /// Whether the named tmux session currently exists.
    async fn has_session(&self, name: &str) -> Result<bool>;

    /// Create the named tmux session, running `command` detached in `workdir`.
    ///
    /// Equivalent to `tmux new-session -d -s <name> -c <workdir> <command...>`.
    /// `command` is passed as an argv vector (not a single shell string), so
    /// arguments such as `claude --resume <id>` are forwarded without
    /// shell-quoting hazards. The caller is responsible for idempotency
    /// (checking [`Self::has_session`] first); this always attempts to create.
    async fn create_session(&self, name: &str, workdir: &str, command: &[String]) -> Result<()>;

    /// Send the given text to `pane` and submit it (Enter).
    ///
    /// `pane` is a fully-qualified tmux target such as `<name>:0.0`; derive it
    /// from a session name with [`crate::ports::tmux_driver::pane_for`].
    async fn send_line(&self, pane: &str, text: &str) -> Result<()>;

    /// Wipe the pane program's current input without submitting anything.
    ///
    /// Clears the input line (via `C-u`) so any residual content is removed.
    /// Used right before a fresh PTY attach so a stray blank line a prior
    /// client's detach left behind does not linger: when the PTY bridge tears
    /// down, tmux delivers a focus-out report (`ESC[O`) to the pane program,
    /// which Claude renders as a blank line in its input box. `pane` is a
    /// fully-qualified tmux target such as `<name>:0.0`.
    async fn clear_input(&self, pane: &str) -> Result<()>;

    /// Kill the named tmux session, terminating its `claude` process.
    ///
    /// Used to close a session: the conversational data persists in the store,
    /// but the live pane and process are gone.
    async fn kill_session(&self, name: &str) -> Result<()>;
}

/// Derive the pane a session's launched command runs in: `<name>:0.0`.
///
/// `tmux new-session` places the launched command in the first pane of the
/// first window, addressed as `<session>:0.0`. The registry and the PTY bridge
/// reuse this so the derivation lives in exactly one place.
pub fn pane_for(name: &str) -> String {
    format!("{name}:0.0")
}

#[async_trait]
impl TmuxDriver for Box<dyn TmuxDriver> {
    async fn has_session(&self, name: &str) -> Result<bool> {
        (**self).has_session(name).await
    }

    async fn create_session(&self, name: &str, workdir: &str, command: &[String]) -> Result<()> {
        (**self).create_session(name, workdir, command).await
    }

    async fn send_line(&self, pane: &str, text: &str) -> Result<()> {
        (**self).send_line(pane, text).await
    }

    async fn clear_input(&self, pane: &str) -> Result<()> {
        (**self).clear_input(pane).await
    }

    async fn kill_session(&self, name: &str) -> Result<()> {
        (**self).kill_session(name).await
    }
}
