//! The `hookSpecificOutput` envelope Claude Code expects for `UserPromptSubmit`.

use serde::Serialize;

/// The `hookSpecificOutput` envelope Claude Code expects for `UserPromptSubmit`.
#[derive(Debug, Serialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "additionalContext")]
    pub additional_context: String,
}
