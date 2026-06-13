//! Claude Code hook handlers: `UserPromptSubmit`, `Stop`, `MessageDisplay`,
//! `PreToolUse`, `PermissionRequest`, `SessionStart`, and `SessionEnd`.

mod bind_pending_spawn;
mod match_uuid_for_prompt;
mod on_message_display;
mod on_permission_request;
mod on_pre_tool_use;
mod on_session_end;
mod on_session_start;
mod on_stop;
mod on_user_prompt_submit;
mod register_on_first_contact;
mod register_session_row;

#[cfg(test)]
mod tests;

/// Claude Code's built-in interactive multiple-choice tool.
///
/// When the assistant calls this tool, Claude Code presents 2–4 options for the
/// user to pick in the TUI. Delta special-cases it across both the `PreToolUse`
/// and `PermissionRequest` hooks (a dedicated question card, not a generic
/// Allow/Deny notice), so the tool name lives here once and both handlers
/// reference it — no magic-string drift.
pub(in crate::interactor) const ASK_USER_QUESTION: &str = "AskUserQuestion";

pub use on_permission_request::PermissionWait;

pub(in crate::interactor::hooks) use match_uuid_for_prompt::match_uuid_for_prompt;
