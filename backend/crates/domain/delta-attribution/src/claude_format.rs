//! Claude Code text-format detection, in one place.
//!
//! These are the textual conventions Claude Code uses on the wire Delta
//! observes (the JSONL transcript and the hook payloads), gathered here so
//! attribution and the hook handlers share one definition. The structural
//! flags (e.g. `is_queued_command`) are already detected by the transcript
//! parser in the gateway; these cover the conventions that are plain strings.

/// Prefix Claude Code writes to the transcript when the user interrupts the
/// in-flight turn. It appears as a `role: user` line whose only text block is
/// either `[Request interrupted by user]` (plain mid-response interrupt) or
/// `[Request interrupted by user for tool use]` (interrupt during a tool use).
/// Matching on the shared prefix covers both variants (and any future suffix)
/// without enumerating each exact string.
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// Prompt prefix Claude Code uses when it injects a background-task
/// completion notification. Such a submission is a harness injection, not a
/// human typing into the pane, so it must not be reported as external input.
const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// Whether a (trimmed) user-line text is the interrupt marker Claude Code
/// writes when the user aborts the in-flight turn.
pub fn is_interrupt_marker(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(INTERRUPT_MARKER_PREFIX)
}

/// Whether a hook-submitted prompt is a harness-injected task notification
/// rather than something typed into the pane.
pub fn is_task_notification(prompt: &str) -> bool {
    prompt.trim_start().starts_with(TASK_NOTIFICATION_PREFIX)
}

/// The `<tool-use-id>` element a `<task-notification>` body carries: the id of
/// the `Agent`/`Task`/`Bash` tool call whose background completion this
/// notification reports. It equals the launching tool_use `id` (the
/// `toolu_...` value), so it is the correlation key from a completion back to
/// the thread that launched the task.
///
/// Returns `None` when the text is not a task notification or carries no
/// `<tool-use-id>` element. The extraction is a minimal element scan rather
/// than a full XML parse: the body is a flat, harness-generated block and the
/// element value never contains markup.
pub fn task_notification_tool_use_id(prompt: &str) -> Option<&str> {
    if !is_task_notification(prompt) {
        return None;
    }
    let open = "<tool-use-id>";
    let close = "</tool-use-id>";
    let start = prompt.find(open)? + open.len();
    let rest = &prompt[start..];
    let end = rest.find(close)?;
    Some(rest[..end].trim())
}

/// Whether a tool_use `input` launches the tool in the background — i.e. it
/// carries the top-level `run_in_background: true` key. Such a call (an
/// `Agent`/`Task`/`Bash` with `run_in_background`) returns immediately and its
/// completion is later injected as a `<task-notification>` user line, so the
/// launching tool_use `id` is recorded as the correlation key for that
/// notification.
pub fn launches_in_background(tool_use_input: &serde_json::Value) -> bool {
    tool_use_input
        .get("run_in_background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_interrupt_marker_variants_match_by_prefix() {
        assert!(is_interrupt_marker("[Request interrupted by user]"));
        assert!(is_interrupt_marker(
            "[Request interrupted by user for tool use]"
        ));
        assert!(!is_interrupt_marker("a normal prompt"));
        assert!(!is_interrupt_marker(""));
    }

    #[test]
    fn task_notification_is_detected_with_leading_whitespace() {
        assert!(is_task_notification("<task-notification>done</task-notification>"));
        assert!(is_task_notification("  <task-notification>done"));
        assert!(!is_task_notification("a normal prompt"));
    }

    #[test]
    fn tool_use_id_is_extracted_from_a_task_notification_body() {
        let body = "<task-notification>\n\
                    <task-id>a31425032172620ed</task-id>\n\
                    <tool-use-id>toolu_01PqcdgEeMZekxvwSqjBviuA</tool-use-id>\n\
                    <output-file>/tmp/x.output</output-file>\n\
                    <status>completed</status>\n\
                    </task-notification>";
        assert_eq!(
            task_notification_tool_use_id(body),
            Some("toolu_01PqcdgEeMZekxvwSqjBviuA")
        );
    }

    #[test]
    fn tool_use_id_extraction_ignores_non_notifications_and_missing_element() {
        // Not a task notification at all.
        assert_eq!(
            task_notification_tool_use_id("<tool-use-id>toolu_x</tool-use-id>"),
            None
        );
        // A notification with no `<tool-use-id>` element (e.g. malformed).
        assert_eq!(
            task_notification_tool_use_id("<task-notification><status>completed</status></task-notification>"),
            None
        );
    }

    #[test]
    fn launches_in_background_reads_the_top_level_flag() {
        assert!(launches_in_background(&serde_json::json!({
            "subagent_type": "general-purpose",
            "run_in_background": true
        })));
        assert!(!launches_in_background(&serde_json::json!({
            "run_in_background": false
        })));
        // Absent key, wrong type, and non-object inputs are all "foreground".
        assert!(!launches_in_background(&serde_json::json!({"command": "ls"})));
        assert!(!launches_in_background(&serde_json::json!({
            "run_in_background": "true"
        })));
        assert!(!launches_in_background(&serde_json::Value::Null));
    }
}
