//! Driving the Claude Code session by sending keystrokes to a tmux pane.

use async_trait::async_trait;

use crate::error::Result;

/// Drives the Claude Code session: manages its tmux session lifecycle and sends
/// keystrokes to its pane.
#[async_trait]
pub trait TmuxDriver: Send + Sync {
    /// Whether the target tmux session currently exists.
    async fn has_session(&self) -> Result<bool>;

    /// Create the target tmux session, running `command` detached in `workdir`.
    ///
    /// Equivalent to `tmux new-session -d -s <session> -c <workdir> <command>`.
    /// The caller is responsible for idempotency (checking [`Self::has_session`]
    /// first); this always attempts to create.
    async fn create_session(&self, workdir: &str, command: &str) -> Result<()>;

    /// Kill the target tmux session if it exists.
    async fn kill_session(&self) -> Result<()>;

    /// Send the given text to the target pane and submit it (Enter).
    async fn send_line(&self, text: &str) -> Result<()>;
}

#[async_trait]
impl TmuxDriver for Box<dyn TmuxDriver> {
    async fn has_session(&self) -> Result<bool> {
        (**self).has_session().await
    }

    async fn create_session(&self, workdir: &str, command: &str) -> Result<()> {
        (**self).create_session(workdir, command).await
    }

    async fn kill_session(&self) -> Result<()> {
        (**self).kill_session().await
    }

    async fn send_line(&self, text: &str) -> Result<()> {
        (**self).send_line(text).await
    }
}
