//! `UserPromptSubmit` response.

use serde::Serialize;

/// `UserPromptSubmit` response.
///
/// Claude Code's HTTP `UserPromptSubmit` hook consumes injected context only
/// from this exact envelope:
///
/// ```json
/// { "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "additionalContext": "<text>" } }
/// ```
///
/// A flat `{ "additionalContext": "..." }` is ignored, so we always wrap the
/// quote in `hookSpecificOutput`. When there is no locator quote to inject the
/// handler returns an empty `200` instead of this body.
#[derive(Debug, Serialize)]
pub struct UserPromptSubmitResponse {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: HookSpecificOutput,
}

impl UserPromptSubmitResponse {
    /// Build a response that injects `additional_context` into the current
    /// prompt.
    pub fn inject(additional_context: String) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "UserPromptSubmit",
                additional_context,
            },
        }
    }
}

/// The `hookSpecificOutput` envelope Claude Code expects for `UserPromptSubmit`.
#[derive(Debug, Serialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "additionalContext")]
    pub additional_context: String,
}
