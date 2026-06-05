//! [`Tmux`]: the concrete [`TmuxDriver`].

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::TmuxDriver;

use crate::error::Error;

/// Drives a Claude Code session living in a specific tmux pane.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// The tmux target pane, e.g. `delta:0.0` or `%3`.
    target_pane: String,
}

impl Tmux {
    /// Create a driver targeting the given tmux pane.
    pub fn new(target_pane: impl Into<String>) -> Self {
        Self {
            target_pane: target_pane.into(),
        }
    }

    async fn run(&self, args: &[&str]) -> std::result::Result<(), Error> {
        let output = Command::new("tmux").args(args).output().await?;
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

#[async_trait]
impl TmuxDriver for Tmux {
    async fn send_line(&self, text: &str) -> std::result::Result<(), delta_usecase::Error> {
        // Send the text literally so it is not interpreted as tmux key names,
        // then submit it with a separate Enter keystroke.
        self.run(&["send-keys", "-t", &self.target_pane, "-l", text])
            .await
            .map_err(delta_usecase::Error::from)?;
        self.run(&["send-keys", "-t", &self.target_pane, "Enter"])
            .await
            .map_err(delta_usecase::Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_target_pane() {
        let t = Tmux::new("delta:0.0");
        assert_eq!(t.target_pane, "delta:0.0");
    }
}
