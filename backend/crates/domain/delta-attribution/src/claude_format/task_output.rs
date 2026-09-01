//! The `TaskOutput` tool and the retrieval report its `tool_result` carries.

use super::{element_inner_text, scan_tool_result_text};

/// The tool Claude Code calls to RETRIEVE a background task's result — the
/// parent's own read of a task it launched earlier, optionally blocking until
/// that task finishes (`block: true`).
///
/// Deliberately NOT part of [`super::SUBAGENT_TOOL_NAMES`]: a retrieval
/// launches nothing, so it must never light a running indicator nor record a
/// launch. It matters for the opposite reason — when the parent consumes a
/// background task's result this way the harness enqueues NO
/// `<task-notification>`, so the successful retrieval is the only signal that
/// can clear the launch's running indicator.
pub const TASK_OUTPUT_TOOL_NAME: &str = "TaskOutput";

/// The `<status>` values that mean the retrieved background task is over and
/// will produce no further output. A retrieval reporting one of these is a
/// completion; `running` — what a non-blocking poll reports for a task still
/// working — is not.
const TERMINAL_TASK_STATUSES: [&str; 3] = ["completed", "failed", "killed"];

/// Whether a task `<status>` value means the task has finished for good (see
/// [`TERMINAL_TASK_STATUSES`]).
pub fn is_terminal_task_status(status: &str) -> bool {
    TERMINAL_TASK_STATUSES.contains(&status)
}

/// The `<status>` a `TaskOutput` retrieval reports for a task that is still
/// working — what a non-blocking poll of an unfinished task returns. Named so
/// the fold can tell "still running, nothing to do" apart from a status
/// Claude Code has drifted to and Delta does not understand.
pub const RUNNING_TASK_STATUS: &str = "running";

/// The element a `TaskOutput` result body opens with (`success` when the
/// retrieval itself worked). Its presence is what identifies a `tool_result`
/// body as a retrieval report at all, so it doubles as the guard for reading
/// the body's other elements.
const RETRIEVAL_STATUS_ELEMENT: &str = "retrieval_status";

/// Whether this `tool_result` content is a `TaskOutput` retrieval report — a
/// body carrying `<retrieval_status>`. Recognizing a retrieval by its own
/// bytes is what lets the fold correlate one whose `TaskOutput` `tool_use`
/// line fell in an EARLIER sync window.
pub fn is_task_output_result(content: &serde_json::Value) -> bool {
    task_output_element(content, RETRIEVAL_STATUS_ELEMENT).is_some()
}

/// The `<status>` element of a `TaskOutput` result body: the state of the
/// RETRIEVED task (`completed`, `failed`, `killed`, or `running` for a
/// non-blocking poll of a task still in flight) — as opposed to
/// `<retrieval_status>`, which reports whether the retrieval itself worked.
///
/// Returns `None` when the content is not a retrieval report or carries no
/// `<status>` element.
pub fn task_output_status(content: &serde_json::Value) -> Option<&str> {
    if !is_task_output_result(content) {
        return None;
    }
    task_output_element(content, "status")
}

/// The `<task_id>` element of a `TaskOutput` result body: the background-task
/// identifier of the task whose result was retrieved. Note the UNDERSCORE —
/// a retrieval report spells it `<task_id>`, where the harness-injected
/// `<task-notification>` spells the same id `<task-id>`.
///
/// Returns `None` when the content is not a retrieval report or carries no
/// `<task_id>` element.
pub fn task_output_task_id(content: &serde_json::Value) -> Option<&str> {
    if !is_task_output_result(content) {
        return None;
    }
    task_output_element(content, "task_id")
}

/// Read one element out of a `tool_result` content payload.
fn task_output_element<'a>(content: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    scan_tool_result_text(content, |text| element_inner_text(text, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_format::{is_subagent_tool, launches_in_background};

    /// A successful `TaskOutput` retrieval report in the array-of-text-blocks
    /// `content` shape — the tolerated alternative to the plain string real
    /// Claude Code writes (covered by the real-shape case below).
    fn task_output_result_content(status: &str) -> serde_json::Value {
        serde_json::json!([{
            "type": "text",
            "text": format!(
                "<retrieval_status>success</retrieval_status>\n\
                 <task_id>a31425032172620ed</task_id>\n\
                 <status>{status}</status>\n\
                 <output>the agent's report</output>"
            ),
        }])
    }

    #[test]
    fn task_output_tool_is_named_but_never_a_launch() {
        // A retrieval is not a launch: it must neither light an indicator nor
        // be classified as a background launch.
        assert!(!is_subagent_tool(TASK_OUTPUT_TOOL_NAME));
        assert!(!launches_in_background(
            TASK_OUTPUT_TOOL_NAME,
            &serde_json::json!({ "task_id": "a31425032172620ed", "block": true }),
        ));
    }

    #[test]
    fn task_output_result_body_yields_its_task_id_and_status() {
        let content = task_output_result_content("completed");
        assert!(is_task_output_result(&content));
        assert_eq!(task_output_task_id(&content), Some("a31425032172620ed"));
        assert_eq!(task_output_status(&content), Some("completed"));
        assert!(is_terminal_task_status("completed"));
        assert!(is_terminal_task_status("failed"));
        assert!(is_terminal_task_status("killed"));
        // A non-blocking poll of a task still working is NOT terminal.
        assert_eq!(
            task_output_status(&task_output_result_content("running")),
            Some("running")
        );
        assert!(!is_terminal_task_status("running"));

        // The shape production actually delivers: `content` is a PLAIN STRING,
        // the elements are blank-line separated, `<task_type>` sits between the
        // id and the status, and the trailing `<output>` is the retrieved
        // agent's own report — which can itself contain these very elements
        // (an agent describing this code does exactly that). The first
        // occurrence wins and every real element precedes `<output>`, so the
        // report's text can never shadow them.
        let real = serde_json::json!(
            "<retrieval_status>success</retrieval_status>\n\n\
             <task_id>a31425032172620ed</task_id>\n\n\
             <task_type>local_agent</task_type>\n\n\
             <status>completed</status>\n\n\
             <output>\nI folded <status>running</status> for <task_id>other</task_id>\n</output>"
        );
        assert!(is_task_output_result(&real));
        assert_eq!(task_output_task_id(&real), Some("a31425032172620ed"));
        assert_eq!(task_output_status(&real), Some("completed"));
    }

    #[test]
    fn task_output_parsers_ignore_bodies_that_are_not_retrieval_reports() {
        // A plain tool_result body: no `<retrieval_status>`, so nothing is a
        // retrieval report — even one that happens to carry a `<status>`.
        let plain = serde_json::json!("<status>completed</status>");
        assert!(!is_task_output_result(&plain));
        assert_eq!(task_output_status(&plain), None);
        assert_eq!(task_output_task_id(&plain), None);
        // A retrieval report missing the elements degrades to `None`.
        let sparse = serde_json::json!("<retrieval_status>success</retrieval_status>");
        assert!(is_task_output_result(&sparse));
        assert_eq!(task_output_status(&sparse), None);
        assert_eq!(task_output_task_id(&sparse), None);
        // Unknown shapes degrade rather than panic.
        assert!(!is_task_output_result(&serde_json::json!(42)));
    }
}
