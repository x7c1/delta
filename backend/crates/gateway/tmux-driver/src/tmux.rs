//! [`Tmux`]: the concrete [`TmuxDriver`].

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::TmuxDriver;

use crate::error::Error;

/// How long to wait after creating the session before the first keystroke, to
/// let the Claude TUI finish initializing so the first `send-keys` is not lost.
const SESSION_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(750);

/// Drives Claude Code sessions living in tmux.
///
/// The driver is stateless with respect to any particular session: every method
/// takes the target session name (or pane) explicitly, so one driver instance
/// manages any number of concurrent sessions. Session names are minted by the
/// caller (Delta's registry), never derived from Claude's `session_id`, so
/// resuming a conversation under a fresh name never collides with a live one.
#[derive(Debug, Clone, Default)]
pub struct Tmux;

impl Tmux {
    /// Create a stateless driver.
    pub fn new() -> Self {
        Self
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
    async fn has_session(&self, name: &str) -> std::result::Result<bool, delta_usecase::Error> {
        // `tmux has-session` exits 0 when the session exists and non-zero when it
        // does not (or the server is not running). A non-zero exit here is the
        // expected "absent" signal, not an error to propagate.
        let output = self
            .output(&["has-session", "-t", name])
            .await
            .map_err(delta_usecase::Error::from)?;
        Ok(output.status.success())
    }

    async fn create_session(
        &self,
        name: &str,
        workdir: &str,
        command: &[String],
    ) -> std::result::Result<(), delta_usecase::Error> {
        let args = new_session_args(name, workdir, command);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&borrowed)
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

    async fn send_line(
        &self,
        pane: &str,
        text: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        for args in send_line_commands(pane, text) {
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.run(&borrowed)
                .await
                .map_err(delta_usecase::Error::from)?;
        }
        Ok(())
    }

    async fn kill_session(&self, name: &str) -> std::result::Result<(), delta_usecase::Error> {
        self.run(&["kill-session", "-t", name])
            .await
            .map_err(delta_usecase::Error::from)
    }
}

/// Build the `tmux new-session` argv that launches `command` detached.
///
/// The launched command is appended as a separate argv tail (not a shell
/// string), so arguments such as `claude --resume <id>` reach the process
/// without shell-quoting hazards.
fn new_session_args(name: &str, workdir: &str, command: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        name.into(),
        "-c".into(),
        workdir.into(),
    ];
    args.extend(command.iter().cloned());
    args
}

/// Build the ordered `tmux send-keys` invocations that submit a single line to
/// `pane`.
///
/// The sequence is clear → literal text → Enter:
/// 1. `C-u` clears the input line first. Claude's TUI input box can retain
///    stray content from a prior submit (e.g. a leftover newline), which would
///    otherwise be prepended to this message. Killing the line makes each
///    programmatic send start from an empty input and be deterministic.
/// 2. `-l <text>` sends the text literally so it is not interpreted as tmux key
///    names.
/// 3. `Enter` submits it as a separate keystroke.
fn send_line_commands(pane: &str, text: &str) -> Vec<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use delta_usecase::pane_for;

    #[test]
    fn pane_for_derives_first_pane_of_session() {
        assert_eq!(pane_for("delta-1"), "delta-1:0.0");
    }

    #[test]
    fn new_session_args_appends_command_as_argv() {
        // A plain spawn forwards just the launch command.
        assert_eq!(
            new_session_args("delta-1", "/work/delta-1", &["claude".to_owned()]),
            vec![
                "new-session",
                "-d",
                "-s",
                "delta-1",
                "-c",
                "/work/delta-1",
                "claude",
            ],
        );
    }

    #[test]
    fn new_session_args_forwards_resume_arguments_unquoted() {
        // A resume forwards `claude --resume <id>` as separate argv entries, so
        // the id never needs shell quoting.
        assert_eq!(
            new_session_args(
                "delta-2",
                "/work/conv",
                &[
                    "claude".to_owned(),
                    "--resume".to_owned(),
                    "abc 123".to_owned()
                ],
            ),
            vec![
                "new-session",
                "-d",
                "-s",
                "delta-2",
                "-c",
                "/work/conv",
                "claude",
                "--resume",
                "abc 123",
            ],
        );
    }

    #[test]
    fn send_line_clears_the_input_before_typing() {
        // The input line must be cleared (`C-u`) before the literal text so a
        // stray newline left by a prior submit cannot prepend to this message.
        // Expected sequence: clear → literal text → Enter, all targeting the
        // passed pane.
        let commands = send_line_commands("delta-1:0.0", "hi");
        assert_eq!(
            commands,
            vec![
                vec!["send-keys", "-t", "delta-1:0.0", "C-u"],
                vec!["send-keys", "-t", "delta-1:0.0", "-l", "hi"],
                vec!["send-keys", "-t", "delta-1:0.0", "Enter"],
            ],
        );
    }
}
