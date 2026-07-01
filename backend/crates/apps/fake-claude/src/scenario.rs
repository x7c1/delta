//! The scenario script: what the fake "model" does, in order.
//!
//! A scenario is a JSON file:
//!
//! ```json
//! {
//!   "session_start": "immediate",
//!   "loop": false,
//!   "steps": [
//!     { "type": "await_prompt" },
//!     { "type": "reply", "text": "scripted reply", "thinking": "optional" },
//!     { "type": "stop" }
//!   ]
//! }
//! ```
//!
//! Top-level fields:
//!
//! - `session_start` (default `"immediate"`): when the launch fires its
//!   `SessionStart` hook. `"skip"` never fires it (a launch that hangs before
//!   becoming ready); `{ "delay_ms": N }` fires it after a scripted delay (a
//!   slow cold start).
//! - `loop` (default `false`): when `true`, the step list restarts from the
//!   top after the last step, so one short script can serve an arbitrarily
//!   long conversation.
//! - `steps`: executed strictly in order. The vocabulary:
//!
//! | step | effect |
//! |---|---|
//! | `await_prompt` | Block until a prompt is submitted (the launch's positional prompt counts), then fire `UserPromptSubmit` and write the user transcript line. |
//! | `reply { text, thinking? }` | Write an assistant transcript line (optional `thinking` block before the text block). `{additional_context}` in `text` substitutes the `additionalContext` the most recent `UserPromptSubmit` hook response injected (empty when none). |
//! | `stream_text { deltas }` | Fire the `MessageDisplay` hook once per entry in `deltas`, mirroring how the real `claude` streams an assistant message's visible text live (before the transcript line lands): the chunks share a fresh `message_id`, carry increasing `index` (0, 1, 2, …), and only the last is `final`. Nothing is written to the transcript — pair it with a following `reply` that persists the full text. |
//! | `tool_use { name, input? }` | Write an assistant `tool_use` line and fire `PreToolUse` with a fresh `tool_use_id`. |
//! | `post_tool_use` | Fire `PostToolUse` for the most recent `tool_use` (its tool name and `tool_use_id`), mirroring how the real `claude` signals a completed tool call. Used to close a subagent's (`Agent`/`Task`) running window without writing a `tool_result`. |
//! | `permission_request { on_allow?, on_deny? }` | Fire `PermissionRequest` for the most recent `tool_use` (an interactive dialog appeared) and BLOCK until the hook responds, exactly like the real `claude` awaiting its permission hook. A decision response (`hookSpecificOutput.decision.behavior`) runs the matching `on_allow`/`on_deny` sub-steps (default empty); an empty passthrough response runs neither — the following steps then play the TUI-answered path. |
//! | `tool_result { is_error? }` | Write the `tool_result` carrier line for the most recent `tool_use`. |
//! | `task_notification { drop_tool_use_id? }` | Write the harness-injected `<task-notification>` completion line for the most recent `tool_use`. The body always includes `<task-id>` (the `agentId` minted at `tool_use` time); `<tool-use-id>` is included by default and omitted when `drop_tool_use_id: true`, modelling the recent Claude Code versions that strip that element from the body. Pair a `tool_use` (with `run_in_background: true` in its input) → `post_tool_use` (the immediate launch ack) → later `task_notification` to model a background subagent's full lifecycle. |
//! | `stop { stop_reason? }` | Fire the `Stop` hook: the turn completed. |
//! | `await_interrupt` | Block until Escape arrives, then write the `[Request interrupted by user]` marker line. No `Stop` fires — exactly like a real interrupt. |
//! | `await_escape` | Block until Escape arrives, writing nothing. Models cancelling an `AskUserQuestion`: a single Escape cancels the call, after which the scenario writes a `tool_result { is_error: true }` for the question — exactly the bytes a real cancel produces. Unlike `await_interrupt` it writes no marker, so the cancel's `tool_result` is the next step. |
//! | `enqueue_prompt { text }` | A prompt submitted while the turn is busy: write the uuid-less `queue-operation` enqueue line carrying the text and remember it for `dequeue_prompt`. No hook fires at enqueue time. |
//! | `dequeue_prompt` | Replay the oldest enqueued prompt now that the turn has freed: fire its own `UserPromptSubmit`, then write it as a plain user line (`promptSource: "queued"`) — the same path a TUI-typed prompt takes. |
//! | `delay { ms }` | Sleep. Only for delays the scenario itself is about (e.g. holding a turn open); synchronization belongs to the `await_*` steps. |
//! | `hang` | Block forever (a launch or turn that never progresses). |
//! | `swallow_prompt` | Consume one prompt from the pane input without firing `UserPromptSubmit` and without writing the transcript — models Claude Code's TUI swallowing the keystroke into the auto-`/compact` routine. The dispatched send stays `Dispatched` behind a missing echo until something re-types it. |
//! | `compact_group` | Write the four-line `/compact` group (caveat + bare command-name + summary + stdout) sharing one `promptId`. The summary line is the `isCompactSummary:true` record that drives `Effect::AutoCompactFinished` on the server. |
//!
//! How the file is found, in priority order:
//!
//! 1. `FAKE_CLAUDE_SCENARIO` — explicit path.
//! 2. `FAKE_CLAUDE_SCENARIO_DIR` — a directory of scenarios; the launch's
//!    positional first prompt selects `<dir>/<first whitespace token>.json`.
//!    This lets one server (whose spawn command is fixed) run a different
//!    scenario per test: the test encodes the scenario name in the prompt it
//!    sends.
//! 3. Neither set — a built-in echo loop (`await_prompt` → `reply` → `stop`,
//!    looping), so a manually launched fake holds a plausible conversation.

use serde::Deserialize;
use serde_json::Value;

/// When the launch fires `SessionStart`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum SessionStartMode {
    /// `"immediate"` or `"skip"`.
    Named(String),
    /// `{ "delay_ms": N }`: fire after a scripted delay.
    Delayed { delay_ms: u64 },
}

impl Default for SessionStartMode {
    fn default() -> Self {
        Self::Named("immediate".to_owned())
    }
}

/// One scripted action. See the module docs for the vocabulary table.
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
    /// Models the auto-`/compact` race: the user's keystroke reaches Claude
    /// Code's TUI just as the compaction routine starts, so the prompt is
    /// swallowed and no echo ever fires. The send Delta dispatched stays
    /// `Dispatched` behind a missing echo until something re-types it.
    SwallowPrompt,
    /// Write the four-line group Claude Code produces for an auto- or
    /// manually-triggered `/compact` (a caveat / command-name / summary /
    /// stdout sequence sharing one `promptId`). The summary line is the
    /// `isCompactSummary:true` record that drives the
    /// `Effect::AutoCompactFinished` re-dispatch.
    CompactGroup,
}

/// A parsed scenario file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Scenario {
    #[serde(default)]
    pub session_start: SessionStartMode,
    /// Restart the step list from the top after the last step.
    #[serde(default, rename = "loop")]
    pub looped: bool,
    pub steps: Vec<Step>,
}

impl Scenario {
    /// Resolve the scenario for this launch. See the module docs for the
    /// priority order; `first_prompt` is the launch's positional prompt, used
    /// for directory-based selection.
    pub fn resolve(first_prompt: Option<&str>) -> Result<Self, String> {
        if let Ok(path) = std::env::var("FAKE_CLAUDE_SCENARIO") {
            return Self::load(&path);
        }
        if let Ok(dir) = std::env::var("FAKE_CLAUDE_SCENARIO_DIR") {
            let token = first_prompt
                .and_then(|p| p.split_whitespace().next())
                .ok_or_else(|| {
                    "FAKE_CLAUDE_SCENARIO_DIR is set but the launch carries no positional \
                        prompt to select a scenario with"
                        .to_owned()
                })?;
            let path = std::path::Path::new(&dir).join(format!("{token}.json"));
            return Self::load(&path.to_string_lossy());
        }
        Ok(Self::echo_loop())
    }

    /// Load and parse a scenario file.
    pub fn load(path: &str) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read scenario {path}: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("parse scenario {path}: {e}"))
    }

    /// The built-in default: answer every prompt with a fixed reply, forever.
    pub fn echo_loop() -> Self {
        Self {
            session_start: SessionStartMode::default(),
            looped: true,
            steps: vec![
                Step::AwaitPrompt,
                Step::Reply {
                    text: "fake-claude scripted reply".to_owned(),
                    thinking: None,
                },
                Step::Stop { stop_reason: None },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_full_vocabulary() {
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "session_start": { "delay_ms": 250 },
                "loop": true,
                "steps": [
                    { "type": "await_prompt" },
                    { "type": "stream_text", "deltas": ["hi", " there"] },
                    { "type": "reply", "text": "hi", "thinking": "hmm" },
                    { "type": "tool_use", "name": "Bash", "input": { "command": "ls" } },
                    { "type": "post_tool_use" },
                    { "type": "permission_request",
                      "on_allow": [ { "type": "tool_result" } ],
                      "on_deny": [ { "type": "reply", "text": "denied" } ] },
                    { "type": "tool_result", "is_error": true },
                    { "type": "task_notification" },
                    { "type": "stop", "stop_reason": "end_turn" },
                    { "type": "await_interrupt" },
                    { "type": "enqueue_prompt", "text": "queued" },
                    { "type": "dequeue_prompt" },
                    { "type": "delay", "ms": 10 },
                    { "type": "hang" },
                    { "type": "swallow_prompt" },
                    { "type": "compact_group" }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(
            scenario.session_start,
            SessionStartMode::Delayed { delay_ms: 250 }
        );
        assert!(scenario.looped);
        assert_eq!(scenario.steps.len(), 16);
        assert_eq!(scenario.steps[0], Step::AwaitPrompt);
        assert_eq!(
            scenario.steps[1],
            Step::StreamText {
                deltas: vec!["hi".to_owned(), " there".to_owned()]
            }
        );
        assert_eq!(scenario.steps[4], Step::PostToolUse);
        assert_eq!(
            scenario.steps[7],
            Step::TaskNotification {
                drop_tool_use_id: false
            }
        );
        assert_eq!(
            scenario.steps[8],
            Step::Stop {
                stop_reason: Some("end_turn".to_owned())
            }
        );
        assert_eq!(scenario.steps[14], Step::SwallowPrompt);
        assert_eq!(scenario.steps[15], Step::CompactGroup);
    }

    #[test]
    fn defaults_are_immediate_session_start_and_no_loop() {
        let scenario: Scenario =
            serde_json::from_str(r#"{ "steps": [ { "type": "await_prompt" } ] }"#).unwrap();
        assert_eq!(scenario.session_start, SessionStartMode::default());
        assert!(!scenario.looped);
    }

    #[test]
    fn skip_parses_as_a_named_mode() {
        let scenario: Scenario =
            serde_json::from_str(r#"{ "session_start": "skip", "steps": [] }"#).unwrap();
        assert_eq!(
            scenario.session_start,
            SessionStartMode::Named("skip".to_owned())
        );
    }
}
