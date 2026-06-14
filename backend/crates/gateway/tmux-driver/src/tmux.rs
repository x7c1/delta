//! [`Tmux`]: the concrete [`TmuxDriver`].

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::TmuxDriver;

use crate::error::Error;

/// Delta's fixed tmux configuration, loaded via `-f` when Delta's server starts.
///
/// Starting the server with `-f <file>` makes tmux skip the user's
/// `~/.tmux.conf` (and the system config) entirely, so the embedded pane behaves
/// identically on every machine no matter how the user has themed or rebound
/// their own tmux. This config is the *only* customization Delta applies, and
/// every line is a deliberate requirement of the embedded pane — not a style
/// preference. See [`Tmux::output`] (which passes `-f`) and
/// [`TmuxDriver::create_session`] (which writes this file before the
/// server-starting `new-session`).
const DELTA_TMUX_CONF: &str = "\
# Delta's fixed tmux configuration. Delta starts its own tmux server with
# `-f <this file>`, which makes tmux ignore the user's ~/.tmux.conf so the
# embedded pane is identical on every machine. Every line below is a deliberate
# requirement of the embedded pane, not a preference.

# Pin the terminal type the Claude pane runs under so its terminfo (and the
# capabilities the TUI probes) are the same everywhere. screen-256color is
# preferred over tmux's own default (tmux-256color) because its terminfo entry
# is present on far more machines out of the box.
set -g default-terminal \"screen-256color\"

# Vanilla tmux holds a lone ESC from a client for 500ms (escape-time) to see
# whether it is the start of an escape sequence. Delta's only attach clients
# are PTY-bridged xterm.js terminals, whose escape sequences always arrive in
# one complete write, so the disambiguation wait buys nothing — it only delays
# Escape (the interrupt key for the Claude TUI) by half a second. Deliver it
# immediately.
set -s escape-time 0

# focus-events is off in vanilla tmux but a common user override turns it on.
# With it on, tmux reports focus in/out to the pane program every time a client
# attaches/detaches (which the embedded terminal does on every session switch),
# and Claude's TUI renders each report as a stray blank line. Pin it off.
set -s focus-events off

# Vanilla tmux shows a status bar; Delta's pane is a permission-answering escape
# hatch, not a full tmux workspace, so the bar only wastes a row and renders the
# user's themed powerline/Nerd-Font glyphs as tofu in the browser xterm.
set -g status off

# Deepen the scrollback so the embedded terminal can scroll far enough back
# through Claude's output to be useful for debugging (via copy-mode: prefix `[`).
# Vanilla tmux keeps only 2000 lines, which a single verbose tool output can
# fill; 10000 lines costs only a few MB per pane.
set -g history-limit 10000
";

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
/// Delta's fixed config (`-f`, see [`DELTA_TMUX_CONF`]) instead of the user's
/// `~/.tmux.conf`, so the embedded pane is identical on every machine.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// The dedicated tmux socket name (`tmux -L <socket>`).
    socket: String,
    /// Path to the rendered [`DELTA_TMUX_CONF`] file passed via `tmux -f`.
    ///
    /// Per-socket so concurrent Delta servers on different sockets never share a
    /// file. Written by [`TmuxDriver::create_session`] before the server starts.
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
    /// starts (by `new-session`, see [`TmuxDriver::create_session`]) and is
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
        // Write Delta's fixed config before the server-starting `new-session`
        // runs. `-f <conf_path>` (added by `output`) is read only when the server
        // boots, and `new-session` is the call that boots it (`has-session` and
        // friends just fail when no server is running). Writing on each create is
        // idempotent: once the server is up the file is left untouched, and a
        // rewrite for an already-running server is a harmless no-op. This is what
        // makes the embedded pane (terminal type, focus events, status bar) the
        // same on every machine — see DELTA_TMUX_CONF.
        tokio::fs::write(&self.conf_path, DELTA_TMUX_CONF)
            .await
            .map_err(|source| Error::Config {
                path: self.conf_path.clone(),
                source,
            })
            .map_err(delta_usecase::Error::from)?;

        let args = new_session_args(name, workdir, command);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&borrowed)
            .await
            .map_err(delta_usecase::Error::from)?;

        // No post-launch settle delay. Readiness is now event-driven via the
        // `SessionStart` hook, which fires when the TUI can actually accept
        // input, so there is no keystroke to race against `new-session`'s return:
        // a fresh spawn submits its first prompt as a launch positional argument
        // (the server never types into a cold pane), and a resume holds its first
        // keystroke until `SessionStart(source=resume)` arrives — measured ~2s
        // after launch, far past the 750ms a fixed settle could safely wait. The
        // 250ms `SUBMIT_ENTER_DELAY` in `send_line` is unrelated (it spaces the
        // submit Enter past Claude's paste-burst window) and stays.
        Ok(())
    }

    async fn send_line(
        &self,
        pane: &str,
        text: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Type the message (clear + literal text) without submitting it.
        for args in input_commands(pane, text) {
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.run(&borrowed)
                .await
                .map_err(delta_usecase::Error::from)?;
        }
        // Wait out Claude's paste-burst window before the submit Enter, so the
        // Enter lands as a discrete keystroke and is not absorbed into the
        // just-typed text (see SUBMIT_ENTER_DELAY).
        tokio::time::sleep(SUBMIT_ENTER_DELAY).await;
        let submit = submit_command(pane);
        let borrowed: Vec<&str> = submit.iter().map(String::as_str).collect();
        self.run(&borrowed)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn send_keys(
        &self,
        pane: &str,
        keys: &[&str],
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Each key is sent as its own discrete `send-keys` invocation with a
        // small settle in between, so the TUI processes one navigation/toggle
        // keystroke at a time. Batching them into one `send-keys` call risks the
        // widget coalescing rapid keys (e.g. a Down+Enter racing the highlight
        // move), which a deliberate human cadence avoids; the settle restores
        // that cadence. The keys come from the pinned key-sequence generator, so
        // they are a fixed vocabulary (`Down`, `Up`, `Space`, `Enter`, …) and
        // never literal text.
        for key in keys {
            let args = key_command(pane, key);
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.run(&borrowed)
                .await
                .map_err(delta_usecase::Error::from)?;
            tokio::time::sleep(KEY_SETTLE).await;
        }
        Ok(())
    }

    async fn clear_input(&self, pane: &str) -> std::result::Result<(), delta_usecase::Error> {
        let args = clear_input_commands(pane);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&borrowed)
            .await
            .map_err(delta_usecase::Error::from)
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

/// How many `BSpace` keystrokes the shared clear sends after `C-u`, to wipe any
/// blank lines stacked above the cursor. `C-u` only kills the current line, so
/// stray blank lines above it survive and would be prepended to the next
/// message. Such a blank line appears when the PTY bridge tears down: tmux does
/// not inject anything itself, but it delivers a focus-out report (`ESC[O`) to
/// the pane program — the embedded terminal enables focus tracking — and
/// Claude's TUI renders that as a stray blank line in its input. This bound is
/// far above any realistic accumulation, and deleting past the start of the
/// input is a harmless no-op, so the clear reliably leaves the input empty.
const INPUT_CLEAR_BACKSPACES: usize = 64;

/// Build the single `tmux send-keys` invocation that wipes the pane program's
/// input: `C-u` kills the current line and a run of `BSpace` (see
/// [`INPUT_CLEAR_BACKSPACES`]) deletes any blank lines stacked above it.
///
/// Two paths need exactly this wipe, so it lives here as one source of truth:
///
/// - Before a programmatic [`send_line_commands`], so a stray newline left by a
///   prior submit cannot prepend to the next message.
/// - On a fresh PTY (re)attach (see [`TmuxDriver::clear_input`]): when the PTY
///   bridge tears down, tmux delivers a focus-out report (`ESC[O`) to the pane
///   program, which Claude renders as a stray blank line in its input. Wiping on
///   the next attach keeps the input box clean across browser reloads.
fn clear_input_commands(pane: &str) -> Vec<String> {
    let mut args = vec!["send-keys".into(), "-t".into(), pane.into(), "C-u".into()];
    args.extend(std::iter::repeat_n(
        "BSpace".to_owned(),
        INPUT_CLEAR_BACKSPACES,
    ));
    args
}

/// How long to wait after typing the literal text before sending the submit
/// `Enter`.
///
/// Claude's TUI treats a fast input burst as a paste; if the `Enter` arrives
/// while that paste-burst window is still open, it is absorbed as part of the
/// pasted text instead of submitting, leaving the message sitting in the input
/// unsent. This is intermittent — it only fails when the `Enter` happens to land
/// inside the window, which is likelier for longer/multi-line text that takes
/// longer to process. Waiting out the window makes the `Enter` a discrete
/// submit. Sized comfortably above the observed window; the added per-send
/// latency is imperceptible. Bumped to 250ms after the occasional miss still
/// slipped through at 150ms (the failure is rare and not reproducible on demand).
const SUBMIT_ENTER_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Build the `tmux send-keys` invocations that type a single line into `pane`'s
/// input **without submitting it**:
///
/// 1. The shared [`clear_input_commands`] wipe (`C-u` then a run of `BSpace`,
///    see [`INPUT_CLEAR_BACKSPACES`]) clears the current line and any blank
///    lines stacked above it, so a prior submit's leftovers are never prepended
///    to this message and each programmatic send starts from an empty input.
/// 2. `-l <text>` sends the text literally so it is not interpreted as tmux key
///    names.
///
/// The submit `Enter` is issued separately by [`submit_command`] after
/// [`SUBMIT_ENTER_DELAY`], so Claude's paste-burst detection cannot absorb it.
fn input_commands(pane: &str, text: &str) -> Vec<Vec<String>> {
    vec![
        clear_input_commands(pane),
        vec![
            "send-keys".into(),
            "-t".into(),
            pane.into(),
            "-l".into(),
            text.into(),
        ],
    ]
}

/// Build the `tmux send-keys` invocation that submits the typed input as a lone
/// `Enter` keystroke. Issued after [`SUBMIT_ENTER_DELAY`] (see [`input_commands`]).
fn submit_command(pane: &str) -> Vec<String> {
    vec!["send-keys".into(), "-t".into(), pane.into(), "Enter".into()]
}

/// How long to settle after each injected navigation/selection keystroke, so
/// the interactive TUI widget processes one key at a time (see [`send_keys`]).
///
/// `claude`'s `AskUserQuestion` widget redraws on each keypress; sending the
/// next key before that redraw can drop a move or coalesce a Down+Enter into a
/// single misfire. This spacing mimics a human cadence. Kept small so a full
/// multi-question answer stays well under a second of injection, but non-zero so
/// the ordering is deterministic.
///
/// [`send_keys`]: TmuxDriver::send_keys
const KEY_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// Build the `tmux send-keys` invocation that sends a single named keystroke
/// (`Down`, `Up`, `Space`, `Enter`, `Right`, `Tab`, …) to `pane`. The key is a
/// tmux key name, not literal text (no `-l`), so the TUI receives it as a real
/// keypress.
fn key_command(pane: &str, key: &str) -> Vec<String> {
    vec!["send-keys".into(), "-t".into(), pane.into(), key.into()]
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
    fn new_session_args_forwards_a_trailing_prompt_argument_unquoted() {
        // A composer-first new session carries the first prompt as a trailing
        // positional argv entry (`claude … <prompt>`), so a multi-line / quoted
        // prompt reaches `claude` verbatim without shell quoting.
        assert_eq!(
            new_session_args(
                "delta-1",
                "/work/delta-1",
                &[
                    "claude".to_owned(),
                    "--settings".to_owned(),
                    "/run/delta/settings.json".to_owned(),
                    "--session-id".to_owned(),
                    "0190-uuid".to_owned(),
                    "hello\nworld \"quoted\"".to_owned(),
                ],
            ),
            vec![
                "new-session",
                "-d",
                "-s",
                "delta-1",
                "-c",
                "/work/delta-1",
                "claude",
                "--settings",
                "/run/delta/settings.json",
                "--session-id",
                "0190-uuid",
                "hello\nworld \"quoted\"",
            ],
        );
    }

    #[test]
    fn input_commands_clear_then_type_without_submitting() {
        // Typing is clear → literal text, with NO Enter: the submit is issued
        // separately after a delay so Claude's paste-burst detection cannot
        // swallow it. `C-u` kills the current line and a run of `BSpace` deletes
        // blank lines stacked above it.
        let commands = input_commands("delta-1:0.0", "hi");

        let mut expected_clear = vec!["send-keys", "-t", "delta-1:0.0", "C-u"];
        expected_clear.extend(std::iter::repeat_n("BSpace", INPUT_CLEAR_BACKSPACES));
        assert_eq!(commands[0], expected_clear);
        assert_eq!(
            &commands[1..],
            &[vec!["send-keys", "-t", "delta-1:0.0", "-l", "hi"]],
        );
    }

    #[test]
    fn submit_command_is_a_lone_enter() {
        // The submit is a single `Enter`, kept separate from typing so it can be
        // delayed past the paste-burst window.
        assert_eq!(
            submit_command("delta-1:0.0"),
            vec!["send-keys", "-t", "delta-1:0.0", "Enter"],
        );
    }

    #[test]
    fn clear_input_commands_wipes_the_input_line_of_the_pane() {
        // The standalone clear is `C-u` followed by a run of `BSpace` (deleting
        // blank lines stacked above the cursor), all targeting the passed pane.
        let mut expected = vec!["send-keys", "-t", "delta-1:0.0", "C-u"];
        expected.extend(std::iter::repeat_n("BSpace", INPUT_CLEAR_BACKSPACES));
        assert_eq!(clear_input_commands("delta-1:0.0"), expected);
    }

    #[test]
    fn input_commands_reuse_the_shared_clear_sequence() {
        // The clear step of typing is the exact same invocation as the
        // standalone clear, so the two paths share one source of truth.
        let pane = "delta-1:0.0";
        assert_eq!(input_commands(pane, "hi")[0], clear_input_commands(pane));
    }

    #[test]
    fn key_command_sends_a_named_keystroke_not_literal_text() {
        // A navigation/selection key is sent by tmux key name (no `-l`), so the
        // TUI receives it as a real keypress rather than typed characters.
        assert_eq!(
            key_command("delta-1:0.0", "Down"),
            vec!["send-keys", "-t", "delta-1:0.0", "Down"],
        );
        assert_eq!(
            key_command("delta-1:0.0", "Enter"),
            vec!["send-keys", "-t", "delta-1:0.0", "Enter"],
        );
    }

    #[test]
    fn conf_path_is_derived_per_socket() {
        // The config path is namespaced by socket so concurrent Delta servers on
        // different sockets never write over each other's config.
        assert!(Tmux::new("delta").conf_path.ends_with("delta-tmux-delta.conf"));
        assert!(Tmux::new("other").conf_path.ends_with("delta-tmux-other.conf"));
    }

    #[test]
    fn fixed_config_pins_the_deliberate_settings() {
        // The whole point of the `-f` config is a host-independent baseline:
        // these lines are the only customization Delta applies, so guard against
        // an edit silently dropping one. (`screen-256color` is pinned over
        // tmux's own default for terminfo portability.)
        assert!(DELTA_TMUX_CONF.contains("set -g default-terminal \"screen-256color\""));
        assert!(DELTA_TMUX_CONF.contains("set -s escape-time 0"));
        assert!(DELTA_TMUX_CONF.contains("set -s focus-events off"));
        assert!(DELTA_TMUX_CONF.contains("set -g status off"));
        assert!(DELTA_TMUX_CONF.contains("set -g history-limit 10000"));
    }
}
