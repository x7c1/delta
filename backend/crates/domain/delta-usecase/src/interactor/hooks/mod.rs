//! Claude Code hook handlers: `UserPromptSubmit`, `Stop`, `MessageDisplay`,
//! `PreToolUse`, `PermissionRequest`, `SessionStart`, and `SessionEnd`.

mod bind_pending_spawn;
mod claude_file_change;
mod follow_relocated_transcript;
mod hook_transcript;
mod match_uuid_for_prompt;
mod on_message_display;
mod on_permission_request;
mod on_post_tool_use;
mod on_pre_tool_use;
mod on_session_end;
mod on_session_start;
mod on_stop;
mod on_user_prompt_submit;
mod register_on_first_contact;
mod register_session_row;
mod validate_transcript_path;

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

/// Whether `tool_name` names a subagent-spawning tool (`Agent`, or the older
/// `Task`). A subagent's own nested tool calls (e.g. its `Bash`) reach the same
/// `PreToolUse`/`PostToolUse` hooks but carry their own tool names, so matching
/// strictly against this set is what keeps an internal tool from flipping the
/// running indicator. The one definition lives with the other Claude Code
/// conventions in [`delta_attribution::claude_format`].
pub(in crate::interactor) use delta_attribution::claude_format::is_subagent_tool;

pub use on_permission_request::PermissionWait;

pub(in crate::interactor) use hook_transcript::HookTranscript;

pub(in crate::interactor::hooks) use match_uuid_for_prompt::match_uuid_for_prompt;
pub(in crate::interactor::hooks) use validate_transcript_path::validate_transcript_path;
