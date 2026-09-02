//! The `tmux` argv vectors the driver runs, as pure builders: every quirk of
//! the embedded pane (the input wipe, bracketed paste, the delayed submit
//! Enter) is expressed here and unit-tested without a tmux server.

/// Build the `tmux new-session` argv that launches `command` detached.
///
/// The launched command is appended as a separate argv tail (not a shell
/// string), so arguments such as `claude --resume <id>` reach the process
/// without shell-quoting hazards.
pub(super) fn new_session_args(name: &str, workdir: &str, command: &[String]) -> Vec<String> {
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
/// - Before a programmatic [`input_commands`], so a stray newline left by a
///   prior submit cannot prepend to the next message.
/// - On a fresh PTY (re)attach (see
///   [`clear_input`](delta_usecase::TmuxDriver::clear_input)): when the PTY
///   bridge tears down, tmux delivers a focus-out report (`ESC[O`) to the pane
///   program, which Claude renders as a stray blank line in its input. Wiping on
///   the next attach keeps the input box clean across browser reloads.
pub(super) fn clear_input_commands(pane: &str) -> Vec<String> {
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
pub(super) const SUBMIT_ENTER_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Build the `tmux send-keys` invocations that type a single line into `pane`'s
/// input **without submitting it**:
///
/// 1. The shared [`clear_input_commands`] wipe (`C-u` then a run of `BSpace`,
///    see [`INPUT_CLEAR_BACKSPACES`]) clears the current line and any blank
///    lines stacked above it, so a prior submit's leftovers are never prepended
///    to this message and each programmatic send starts from an empty input.
/// 2. `-l <text>` sends the text literally, wrapped in xterm bracketed-paste
///    markers (`ESC [ 200 ~` … `ESC [ 201 ~`), so the TUI treats the bytes as
///    paste content rather than as keystrokes.
///
/// The submit `Enter` is issued separately by [`submit_command`] after
/// [`SUBMIT_ENTER_DELAY`], so Claude's paste-burst detection cannot absorb it,
/// and stays *outside* the bracketed-paste region.
///
/// Why bracketed paste: outside paste mode, Claude's TUI input widget
/// normalizes each embedded LF (0x0a) in typed input to a single space. A
/// multi-line prompt typed via `send-keys -l` therefore reaches Claude as
/// space-joined text, and echoes back via the `UserPromptSubmit` hook in that
/// same form, which never equals the outstanding send's original
/// `\n`-containing text. Correlation survives that — the prompt consumes the
/// send by position, and the first line ingested afterwards is attributed to
/// the send's thread whatever it says — so what the flattening costs is the
/// message itself: the line breaks the user wrote never reach Claude. It also
/// fails the verbatim check on every multi-line send, which logs each one as a
/// rewritten prompt and withholds the extras keyed on that verdict (the
/// locator-quote frame, the `TurnStarted` naming the matched uuid). Wrapping
/// the payload in `ESC [ 200 ~` … `ESC [ 201 ~` tells the TUI "this is a
/// paste", which preserves embedded LFs verbatim and makes the hook echo match.
pub(super) fn input_commands(pane: &str, text: &str) -> Vec<Vec<String>> {
    vec![
        clear_input_commands(pane),
        vec![
            "send-keys".into(),
            "-t".into(),
            pane.into(),
            "-l".into(),
            format!("{BRACKETED_PASTE_START}{text}{BRACKETED_PASTE_END}"),
        ],
    ]
}

/// xterm bracketed-paste start marker (`ESC [ 200 ~`). Tells the receiving TUI
/// that the bytes that follow are paste content, not keystrokes — preserving
/// embedded LFs that would otherwise be normalized to spaces by the input
/// widget. See [`input_commands`] for the bug this guards against.
const BRACKETED_PASTE_START: &str = "\x1b[200~";

/// xterm bracketed-paste end marker (`ESC [ 201 ~`); the paired terminator of
/// [`BRACKETED_PASTE_START`]. See [`input_commands`].
const BRACKETED_PASTE_END: &str = "\x1b[201~";

/// Build the `tmux send-keys` invocation that submits the typed input as a lone
/// `Enter` keystroke. Issued after [`SUBMIT_ENTER_DELAY`] (see [`input_commands`]).
pub(super) fn submit_command(pane: &str) -> Vec<String> {
    vec!["send-keys".into(), "-t".into(), pane.into(), "Enter".into()]
}

/// How long to settle after each injected navigation/selection keystroke, so
/// the interactive TUI widget processes one key at a time (see
/// [`send_keys`](delta_usecase::TmuxDriver::send_keys)).
///
/// `claude`'s `AskUserQuestion` widget redraws on each keypress; sending the
/// next key before that redraw can drop a move or coalesce a Down+Enter into a
/// single misfire. This spacing mimics a human cadence. Kept small so a full
/// multi-question answer stays well under a second of injection, but non-zero so
/// the ordering is deterministic.
pub(super) const KEY_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// Build the `tmux send-keys` invocation that sends a single named keystroke
/// (`Down`, `Up`, `Space`, `Enter`, `Right`, `Tab`, …) to `pane`. The key is a
/// tmux key name, not literal text (no `-l`), so the TUI receives it as a real
/// keypress.
pub(super) fn key_command(pane: &str, key: &str) -> Vec<String> {
    vec!["send-keys".into(), "-t".into(), pane.into(), key.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // blank lines stacked above it. The literal text is wrapped in
        // bracketed-paste markers so the TUI preserves embedded LFs.
        let commands = input_commands("delta-1:0.0", "hi");

        let mut expected_clear = vec!["send-keys", "-t", "delta-1:0.0", "C-u"];
        expected_clear.extend(std::iter::repeat_n("BSpace", INPUT_CLEAR_BACKSPACES));
        assert_eq!(commands[0], expected_clear);
        assert_eq!(
            &commands[1..],
            &[vec![
                "send-keys",
                "-t",
                "delta-1:0.0",
                "-l",
                "\x1b[200~hi\x1b[201~",
            ]],
        );
    }

    #[test]
    fn input_commands_wraps_multiline_text_in_bracketed_paste_markers() {
        // The regression case: a multi-line prompt must reach the TUI as paste
        // content so the embedded LF is preserved verbatim. Without the
        // wrapping markers the TUI normalizes the LF to a single space, the
        // UserPromptSubmit echo no longer matches the outstanding send, and
        // the send is requeued forever. The wrapped payload is bytewise
        // `ESC [ 200 ~ line one \n line two ESC [ 201 ~`.
        let commands = input_commands("delta-1:0.0", "line one\nline two");

        assert_eq!(
            &commands[1..],
            &[vec![
                "send-keys",
                "-t",
                "delta-1:0.0",
                "-l",
                "\x1b[200~line one\nline two\x1b[201~",
            ]],
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
}
