//! One scripted action.

use serde::Deserialize;
use serde_json::Value;

/// One scripted action. See the `scenario` module docs for the vocabulary
/// table.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    AwaitPrompt,
    Reply {
        text: String,
        #[serde(default)]
        thinking: Option<String>,
    },
    StreamText {
        /// One visible text chunk per `MessageDisplay` fire, in order. The last
        /// chunk is marked `final`.
        deltas: Vec<String>,
    },
    ToolUse {
        name: String,
        #[serde(default)]
        input: Value,
    },
    PostToolUse,
    PermissionRequest {
        #[serde(default)]
        on_allow: Vec<Step>,
        #[serde(default)]
        on_deny: Vec<Step>,
    },
    ToolResult {
        #[serde(default)]
        is_error: bool,
    },
    TaskNotification {
        /// When `true`, the emitted `<task-notification>` body omits its
        /// `<tool-use-id>` element — modelling the recent Claude Code versions
        /// that strip it from the user-message body while keeping `<task-id>`.
        /// The default (`false`) keeps the historical shape with both elements.
        #[serde(default)]
        drop_tool_use_id: bool,
    },
    /// The parent reading the most recent background `tool_use`'s result via
    /// the `TaskOutput` tool, rather than waiting for a `<task-notification>`
    /// (which the harness then never injects at all).
    TaskOutput {
        /// The `<status>` the retrieval reports for the retrieved task:
        /// `completed` (the default), `failed`, or `killed` for a finished
        /// task, `running` for a non-blocking poll of one still working.
        #[serde(default = "default_task_output_status")]
        status: String,
    },
    Stop {
        #[serde(default)]
        stop_reason: Option<String>,
    },
    AwaitInterrupt,
    AwaitEscape,
    EnqueuePrompt {
        text: String,
    },
    DequeuePrompt,
    Delay {
        ms: u64,
    },
    Hang,
    /// Consume one prompt from the pane input without firing
    /// `UserPromptSubmit` and without writing anything to the transcript.
    ///
    /// Models every way Claude Code's TUI can eat a keystroke whole, which
    /// look identical from Delta's side — no echo ever fires and the
    /// dispatched send stays `Dispatched` until something re-types it:
    ///
    /// - the auto-`/compact` race (the keystroke arrives just as the
    ///   compaction routine starts), which the compact re-dispatch recovers
    ///   once the summary lands;
    /// - an interactive dialog Claude Code raised on its own between turns,
    ///   which swallows the pasted text and answers itself with the trailing
    ///   Enter, leaving NO signal at all — only the echo-deadline watchdog
    ///   recovers that one, so a scenario models it by swallowing and then
    ///   simply not writing anything.
    ///
    /// Two consecutive steps swallow the watchdog's re-type as well, which is
    /// how a scenario drives the send all the way to being parked.
    SwallowPrompt,
    /// Consume one prompt from the pane input without firing any hook and
    /// without writing anything to the transcript: a local slash command
    /// (`/cost`, `/model`, …) as Claude Code 2.1.286 and later runs it. The
    /// TUI answers the command itself and records nothing — no
    /// `UserPromptSubmit`, no `Stop`, and none of the caveat / command-name /
    /// stdout group earlier versions wrote.
    ///
    /// Observably identical to [`Step::SwallowPrompt`]; the separate name
    /// says which story the scenario tells, because Delta must read the two
    /// silences oppositely — a swallowed prompt is re-typed, a local command
    /// that ran must not be.
    LocalCommand {
        /// When `true`, the command does not print and exit but leaves a
        /// dialog open, as `/cost` (the usage panel) and `/model` (the model
        /// picker) do on 2.1.286: the step then blocks until Escape arrives,
        /// and every prompt typed into the pane meanwhile is dropped — the
        /// dialog swallows it. The default (`false`) is a command that leaves
        /// the TUI at its prompt.
        #[serde(default)]
        opens_dialog: bool,
    },
    /// Write the four-line group Claude Code produces for an auto- or
    /// manually-triggered `/compact` (a caveat / command-name / summary /
    /// stdout sequence sharing one `promptId`). The group lands atomically —
    /// one append, one shared timestamp — like the real transcript's. The
    /// summary line is the `isCompactSummary:true` record that drives the
    /// `Effect::AutoCompactFinished` re-dispatch.
    CompactGroup,
    /// Move the transcript to a new project directory, the way Claude Code
    /// does when the session enters a worktree (see the module docs).
    Relocate {
        /// The worktree's name: both the new working directory under the
        /// current one and the project directory the transcript moves to.
        dir: String,
    },
}

/// The status a `task_output` step reports when the scenario does not say:
/// the retrieved background task finished normally.
fn default_task_output_status() -> String {
    "completed".to_owned()
}
