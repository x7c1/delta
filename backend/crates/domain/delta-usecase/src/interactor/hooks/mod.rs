//! Claude Code hook handlers: `UserPromptSubmit`, `Stop`, `MessageDisplay`,
//! `PreToolUse`, `PermissionRequest`, `SessionStart`, and `SessionEnd`.

mod bind_pending_spawn;
mod hook_transcript_guard;
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

/// The tool names that spawn a subagent.
///
/// The current Claude Code build (probed on v2.1.177) names this tool `Agent`;
/// older builds named it `Task`. The hook contract is a drift surface, so both
/// are matched defensively. A subagent's own nested tool calls (e.g. its
/// `Bash`) reach the same `PreToolUse`/`PostToolUse` hooks but carry their own
/// tool names, so matching strictly against this set is what keeps an internal
/// tool from flipping the running indicator.
pub(in crate::interactor) const SUBAGENT_TOOL_NAMES: [&str; 2] = ["Agent", "Task"];

/// Whether `tool_name` names a subagent-spawning tool (see
/// [`SUBAGENT_TOOL_NAMES`]).
pub(in crate::interactor) fn is_subagent_tool(tool_name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&tool_name)
}

pub use on_permission_request::PermissionWait;

pub(in crate::interactor::hooks) use match_uuid_for_prompt::match_uuid_for_prompt;
pub(in crate::interactor::hooks) use validate_transcript_path::validate_transcript_path;
