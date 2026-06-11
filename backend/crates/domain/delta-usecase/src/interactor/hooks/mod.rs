//! Claude Code hook handlers: `UserPromptSubmit`, `Stop`, `PreToolUse`,
//! `PermissionRequest`, `SessionStart`, and `SessionEnd`.

mod bind_pending_spawn;
mod match_uuid_for_prompt;
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

pub(in crate::interactor::hooks) use match_uuid_for_prompt::match_uuid_for_prompt;
