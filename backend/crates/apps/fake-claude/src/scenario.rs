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
//! | `tool_use { name, input? }` | Write an assistant `tool_use` line and fire `PreToolUse` with a fresh `tool_use_id`. |
//! | `permission_request` | Fire `PermissionRequest` for the most recent `tool_use` (an interactive dialog appeared). |
//! | `tool_result { is_error? }` | Write the `tool_result` carrier line for the most recent `tool_use`. |
//! | `stop { stop_reason? }` | Fire the `Stop` hook: the turn completed. |
//! | `await_interrupt` | Block until Escape arrives, then write the `[Request interrupted by user]` marker line. No `Stop` fires — exactly like a real interrupt. |
//! | `write_queued_command { text }` | Write a `queued_command` attachment line (a prompt queued while the turn was busy). |
//! | `delay { ms }` | Sleep. Only for delays the scenario itself is about (e.g. holding a turn open); synchronization belongs to the `await_*` steps. |
//! | `hang` | Block forever (a launch or turn that never progresses). |
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
    ToolUse {
        name: String,
        #[serde(default)]
        input: Value,
    },
    PermissionRequest,
    ToolResult {
        #[serde(default)]
        is_error: bool,
    },
    Stop {
        #[serde(default)]
        stop_reason: Option<String>,
    },
    AwaitInterrupt,
    WriteQueuedCommand {
        text: String,
    },
    Delay {
        ms: u64,
    },
    Hang,
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
                .ok_or_else(
                    || "FAKE_CLAUDE_SCENARIO_DIR is set but the launch carries no positional \
                        prompt to select a scenario with"
                        .to_owned(),
                )?;
            let path = std::path::Path::new(&dir).join(format!("{token}.json"));
            return Self::load(&path.to_string_lossy());
        }
        Ok(Self::echo_loop())
    }

    /// Load and parse a scenario file.
    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("read scenario {path}: {e}"))?;
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
                    { "type": "reply", "text": "hi", "thinking": "hmm" },
                    { "type": "tool_use", "name": "Bash", "input": { "command": "ls" } },
                    { "type": "permission_request" },
                    { "type": "tool_result", "is_error": true },
                    { "type": "stop", "stop_reason": "end_turn" },
                    { "type": "await_interrupt" },
                    { "type": "write_queued_command", "text": "queued" },
                    { "type": "delay", "ms": 10 },
                    { "type": "hang" }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(
            scenario.session_start,
            SessionStartMode::Delayed { delay_ms: 250 }
        );
        assert!(scenario.looped);
        assert_eq!(scenario.steps.len(), 10);
        assert_eq!(scenario.steps[0], Step::AwaitPrompt);
        assert_eq!(
            scenario.steps[5],
            Step::Stop {
                stop_reason: Some("end_turn".to_owned())
            }
        );
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
