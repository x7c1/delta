//! Claude Code text-format detection, in one place.
//!
//! These are the textual conventions Claude Code uses on the wire Delta
//! observes (the JSONL transcript and the hook payloads), gathered here so
//! attribution and the hook handlers share one definition. The structural
//! flags (e.g. `is_queued_command`) are already detected by the transcript
//! parser in the gateway; these cover the conventions that are plain strings.

mod forked_skill_launch;
pub use forked_skill_launch::{forked_skill_launch, has_forked_skill_launch, ForkedSkillLaunch};

mod interrupt_marker;
pub use interrupt_marker::is_interrupt_marker;

mod local_command;
pub use local_command::{
    is_local_command_caveat, is_local_command_output, is_slash_command_send,
    local_command_name_line_matches_send,
};

mod pasted_content;
pub use pasted_content::unwrap_pasted_content;

mod prompt_echo;
pub use prompt_echo::prompt_echoes_send;

mod subagent_launch;
pub use subagent_launch::{
    agent_id_from_tool_result_content, is_subagent_tool, launches_in_background,
    SUBAGENT_TOOL_NAMES,
};

mod task_notification;
pub use task_notification::{
    is_task_notification, task_notification_task_id, task_notification_tool_use_id,
};

mod task_output;
pub use task_output::{
    is_task_output_result, is_terminal_task_status, task_output_status, task_output_task_id,
    RUNNING_TASK_STATUS, TASK_OUTPUT_TOOL_NAME,
};

mod unknown_command;
pub use unknown_command::{is_unknown_command_notice, unknown_command_from_notice};

/// The inner text of the first `<name>...</name>` element in `text`, trimmed.
///
/// A minimal element scan, not an XML parse: every body it is used on (the
/// harness-injected `<task-notification>`, a `TaskOutput` retrieval report) is
/// a flat, generated block whose element values never contain markup.
fn element_inner_text<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim())
}

/// Apply `scan` to each text a `tool_result` `content` payload carries,
/// returning the first hit.
///
/// Accepts the three shapes `content` realistically takes: an array of
/// `{ "type": "text", "text": "..." }` blocks (the typical Claude shape), a
/// single such object, or a plain JSON string. Anything else degrades to
/// `None`.
fn scan_tool_result_text<'a>(
    content: &'a serde_json::Value,
    scan: impl Fn(&'a str) -> Option<&'a str>,
) -> Option<&'a str> {
    if let Some(text) = content.as_str() {
        return scan(text);
    }
    if let Some(blocks) = content.as_array() {
        return blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
            .find_map(scan);
    }
    content
        .get("text")
        .and_then(serde_json::Value::as_str)
        .and_then(scan)
}

/// Read an optional non-empty string field out of a JSON payload — an
/// `Agent`/`Task` tool input, or a `<forked-skill-launch>` body.
///
/// Returns `None` when the payload is not an object, the key is missing, the
/// value is not a string, or the string is empty — so a malformed or partial
/// payload degrades to "no label" rather than failing.
pub fn json_string_field<'a>(payload: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    payload.get(key)?.as_str().filter(|s| !s.is_empty())
}
