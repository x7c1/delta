//! [`Tmux`]: the concrete [`TmuxDriver`].

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::TmuxDriver;

use crate::error::Error;

/// How long to wait after creating the session before the first keystroke, to
/// let the Claude TUI finish initializing so the first `send-keys` is not lost.
const SESSION_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(750);

/// Drives a Claude Code session living in a tmux session.
///
/// The driver owns a session *name* (e.g. `delta`) and derives the pane it sends
/// keystrokes to as `<session>:0.0` — the first pane of the first window, which
/// is where `tmux new-session` places the launched command. Keeping the session
/// name (rather than a bare pane string) lets the driver manage the session
/// lifecycle (`has_session`/`create_session`) in addition to driving the pane.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// The tmux session name, e.g. `delta`.
    session: String,
    /// The derived target pane, `<session>:0.0`.
    target_pane: String,
}

impl Tmux {
    /// Create a driver managing the tmux session with the given name.
    pub fn new(session: impl Into<String>) -> Self {
        let session = session.into();
        let target_pane = format!("{session}:0.0");
        Self {
            session,
            target_pane,
        }
    }

    /// The pane this driver sends keystrokes to and attaches the PTY bridge to.
    pub fn target_pane(&self) -> &str {
        &self.target_pane
    }

    /// Run `tmux <args>`, returning the captured output for inspection.
    async fn output(&self, args: &[&str]) -> std::result::Result<std::process::Output, Error> {
        Ok(Command::new("tmux").args(args).output().await?)
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

#[async_trait]
impl TmuxDriver for Tmux {
    async fn has_session(&self) -> std::result::Result<bool, delta_usecase::Error> {
        // `tmux has-session` exits 0 when the session exists and non-zero when it
        // does not (or the server is not running). A non-zero exit here is the
        // expected "absent" signal, not an error to propagate.
        let output = self
            .output(&["has-session", "-t", &self.session])
            .await
            .map_err(delta_usecase::Error::from)?;
        Ok(output.status.success())
    }

    async fn create_session(
        &self,
        workdir: &str,
        command: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.run(&[
            "new-session",
            "-d",
            "-s",
            &self.session,
            "-c",
            workdir,
            command,
        ])
        .await
        .map_err(delta_usecase::Error::from)?;

        // Settle delay: immediately after `tmux new-session ... claude`, the
        // Claude TUI has not finished initializing its terminal, so the very
        // first `send-keys` can be swallowed. We cannot screen-scrape the TUI to
        // detect readiness, so we wait a short fixed interval before returning.
        // This is intentionally on the create path only — reused sessions are
        // already up and pay no delay. A keystroke that still arrives too early
        // remains visible/answerable in the embedded terminal.
        tokio::time::sleep(SESSION_SETTLE_DELAY).await;
        Ok(())
    }

    async fn send_line(&self, text: &str) -> std::result::Result<(), delta_usecase::Error> {
        for args in self.send_line_commands(text) {
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.run(&borrowed)
                .await
                .map_err(delta_usecase::Error::from)?;
        }
        Ok(())
    }
}

impl Tmux {
    /// Build the ordered `tmux send-keys` invocations that submit a single line.
    ///
    /// The sequence is clear → literal text → Enter:
    /// 1. `C-u` clears the input line first. Claude's TUI input box can retain
    ///    stray content from a prior submit (e.g. a leftover newline), which
    ///    would otherwise be prepended to this message. Killing the line makes
    ///    each programmatic send start from an empty input and be deterministic.
    /// 2. `-l <text>` sends the text literally so it is not interpreted as tmux
    ///    key names.
    /// 3. `Enter` submits it as a separate keystroke.
    fn send_line_commands(&self, text: &str) -> Vec<Vec<String>> {
        let pane = self.target_pane.as_str();
        vec![
            vec!["send-keys".into(), "-t".into(), pane.into(), "C-u".into()],
            vec![
                "send-keys".into(),
                "-t".into(),
                pane.into(),
                "-l".into(),
                text.into(),
            ],
            vec!["send-keys".into(), "-t".into(), pane.into(), "Enter".into()],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pane_from_session_name() {
        let t = Tmux::new("delta");
        assert_eq!(t.session, "delta");
        assert_eq!(t.target_pane(), "delta:0.0");
    }

    #[test]
    fn send_line_clears_the_input_before_typing() {
        // The input line must be cleared (`C-u`) before the literal text so a
        // stray newline left by a prior submit cannot prepend to this message.
        // Expected sequence: clear → literal text → Enter, all targeting the
        // session's first pane.
        let t = Tmux::new("delta");
        let commands = t.send_line_commands("hi");
        assert_eq!(
            commands,
            vec![
                vec!["send-keys", "-t", "delta:0.0", "C-u"],
                vec!["send-keys", "-t", "delta:0.0", "-l", "hi"],
                vec!["send-keys", "-t", "delta:0.0", "Enter"],
            ],
        );
    }
}
