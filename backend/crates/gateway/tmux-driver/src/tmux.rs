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
///
/// Every command runs against Delta's **own tmux server** via a dedicated socket
/// (`tmux -L <socket>`), kept separate from the user's default tmux server. This
/// isolation means Delta's sessions never clutter the user's `tmux ls`, teardown
/// can kill the whole server at once, and server-wide options Delta sets (e.g.
/// `focus-events off`, see [`TmuxDriver::create_session`]) never affect the
/// user's other tmux sessions.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// The dedicated tmux socket name (`tmux -L <socket>`).
    socket: String,
}

impl Tmux {
    /// Create a driver bound to a dedicated tmux socket.
    pub fn new(socket: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    /// Run `tmux -L <socket> <args>`, returning the captured output.
    ///
    /// The `-L <socket>` prefix pins every command to Delta's own tmux server.
    async fn output(&self, args: &[&str]) -> std::result::Result<std::process::Output, Error> {
        Ok(Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
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

        // Turn off focus events on Delta's tmux server. `focus-events` is a
        // server-wide option; with it on (a common user default, inherited from
        // ~/.tmux.conf) tmux sends the pane's program a focus in/out report each
        // time a client attaches/detaches — which the embedded terminal does on
        // every session switch — and Claude's TUI renders each as a stray blank
        // line. The primary fix is the frontend dropping these terminal device
        // reports before they reach the pane (see the PTY terminal's input
        // filter); disabling focus events here is complementary defense so tmux
        // never generates them in the first place. Safe because Delta runs on its
        // own socket, so this never affects the user's other tmux sessions.
        // Idempotent, so setting it on each create is harmless.
        self.run(&["set-option", "-s", "focus-events", "off"])
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

/// How many leading `BSpace` keystrokes to send before typing, to wipe any
/// blank lines that accumulated above the cursor. `C-u` only kills the current
/// line, so stray blank lines stacked above it survive and would be prepended to
/// the message. tmux injects one such blank line into the pane's program each
/// time a terminal client detaches; the embedded terminal holds a persistent
/// attach per session to avoid that, but a residual line can still appear (e.g.
/// when the terminal pane is closed, detaching every session). This bound is far
/// above any realistic accumulation, and deleting past the start of the input is
/// a harmless no-op, so it reliably leaves the input empty before typing.
const INPUT_CLEAR_BACKSPACES: usize = 64;

/// Build the ordered `tmux send-keys` invocations that submit a single line to
/// `pane`.
///
/// The sequence is clear → literal text → Enter:
/// 1. `C-u` kills the current input line, then a run of `BSpace` deletes any
///    blank lines stacked above it (see [`INPUT_CLEAR_BACKSPACES`]). Together
///    they leave the input empty so a prior submit's leftovers are never
///    prepended to this message, making each programmatic send deterministic.
/// 2. `-l <text>` sends the text literally so it is not interpreted as tmux key
///    names.
/// 3. `Enter` submits it as a separate keystroke.
fn send_line_commands(pane: &str, text: &str) -> Vec<Vec<String>> {
    let mut clear = vec!["send-keys".into(), "-t".into(), pane.into(), "C-u".into()];
    clear.extend(std::iter::repeat_n("BSpace".to_owned(), INPUT_CLEAR_BACKSPACES));
    vec![
        clear,
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
        // The input must be cleared before the literal text so stray blank lines
        // left by a prior submit cannot prepend to this message: `C-u` kills the
        // current line and a run of `BSpace` deletes blank lines stacked above
        // it. Expected sequence: clear → literal text → Enter, all targeting the
        // passed pane.
        let commands = send_line_commands("delta-1:0.0", "hi");

        let mut expected_clear = vec!["send-keys", "-t", "delta-1:0.0", "C-u"];
        expected_clear.extend(std::iter::repeat_n("BSpace", INPUT_CLEAR_BACKSPACES));
        assert_eq!(commands[0], expected_clear);
        assert_eq!(
            &commands[1..],
            &[
                vec!["send-keys", "-t", "delta-1:0.0", "-l", "hi"],
                vec!["send-keys", "-t", "delta-1:0.0", "Enter"],
            ],
        );
    }
}
