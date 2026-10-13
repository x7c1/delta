//! The `<task-notification>` prompt Claude Code injects when a background task
//! completes.

use super::element_inner_text;

/// Prompt prefix Claude Code uses when it injects a background-task
/// completion notification. Such a submission is a harness injection, not a
/// human typing into the pane, so it must not be reported as external input.
const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

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
    task_notification_element(prompt, "tool-use-id")
}

/// The `<task-id>` element a `<task-notification>` body carries: the
/// background-task identifier Claude Code mints for the subagent, separate
/// from the launching `<tool-use-id>`. Recent Claude Code versions sometimes
/// drop `<tool-use-id>` from the user-message notification body while keeping
/// `<task-id>`, so it serves as a fallback correlation key when matching a
/// completion back to its launching `RunningSubagent` entry.
///
/// Returns `None` when the text is not a task notification or carries no
/// `<task-id>` element. Mirrors [`task_notification_tool_use_id`]: a minimal
/// element scan over the flat, harness-generated body.
pub fn task_notification_task_id(prompt: &str) -> Option<&str> {
    task_notification_element(prompt, "task-id")
}

/// Inner-text extractor shared by [`task_notification_tool_use_id`] and
/// [`task_notification_task_id`]. The body is gated on the task-notification
/// prefix, then scanned for `<name>...</name>`.
fn task_notification_element<'a>(prompt: &'a str, name: &str) -> Option<&'a str> {
    if !is_task_notification(prompt) {
        return None;
    }
    element_inner_text(prompt, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_notification_is_detected_with_leading_whitespace() {
        assert!(is_task_notification(
            "<task-notification>done</task-notification>"
        ));
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
            task_notification_tool_use_id(
                "<task-notification><status>completed</status></task-notification>"
            ),
            None
        );
    }

    #[test]
    fn task_id_is_extracted_from_a_task_notification_body() {
        let body = "<task-notification>\n\
                    <task-id>a31425032172620ed</task-id>\n\
                    <tool-use-id>toolu_01PqcdgEeMZekxvwSqjBviuA</tool-use-id>\n\
                    <output-file>/tmp/x.output</output-file>\n\
                    <status>completed</status>\n\
                    </task-notification>";
        assert_eq!(task_notification_task_id(body), Some("a31425032172620ed"));
    }

    #[test]
    fn task_id_is_extracted_when_tool_use_id_is_missing() {
        // The motivating case: recent Claude Code versions sometimes drop
        // `<tool-use-id>` from the body while keeping `<task-id>`. The fallback
        // key must still come back here.
        let body = "<task-notification>\n\
                    <task-id>a31425032172620ed</task-id>\n\
                    <status>completed</status>\n\
                    </task-notification>";
        assert_eq!(
            task_notification_tool_use_id(body),
            None,
            "the tool-use-id element really is absent"
        );
        assert_eq!(task_notification_task_id(body), Some("a31425032172620ed"));
    }

    #[test]
    fn task_id_extraction_ignores_non_notifications_and_missing_element() {
        assert_eq!(
            task_notification_task_id("<task-id>a31425032172620ed</task-id>"),
            None
        );
        assert_eq!(
            task_notification_task_id(
                "<task-notification><status>completed</status></task-notification>"
            ),
            None
        );
    }
}
