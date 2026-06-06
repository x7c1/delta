//! Driving the Claude Code session by sending keystrokes to a tmux pane.

use async_trait::async_trait;

use crate::error::Result;

/// Drives the Claude Code session by sending keystrokes to a tmux pane.
#[async_trait]
pub trait TmuxDriver: Send + Sync {
    /// Send the given text to the target pane and submit it (Enter).
    async fn send_line(&self, text: &str) -> Result<()>;
}

#[async_trait]
impl TmuxDriver for Box<dyn TmuxDriver> {
    async fn send_line(&self, text: &str) -> Result<()> {
        (**self).send_line(text).await
    }
}
