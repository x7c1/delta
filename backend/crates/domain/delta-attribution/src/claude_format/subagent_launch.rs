//! Tool calls that launch a subagent or a background task, and the id their
//! result carries.

use super::scan_tool_result_text;

/// Fallback used during fold to capture the `agentId: <id>` substring from
/// the launch tool_result text, because the JSONL sibling
/// `toolUseResult.agentId` is not preserved in `ContentBlock::ToolResult`.
///
/// Claude Code writes the background-task identifier into the human-readable
/// `tool_result` text (`Async agent launched successfully.\nagentId: <id> ...`)
/// alongside a sibling `toolUseResult.agentId` field on the same JSONL line.
/// The sibling carries the same value, but the structural parser only keeps
/// the `content` blocks, so the in-memory `tool_result` Delta sees has only
/// the text. Recovering the id from that text gives the fold path the same
/// `task_id` upgrade the live `PostToolUse(Agent)` hook records — needed when
/// a `<task-notification>` body ships only `<task-id>`.
///
/// Accepts every shape `scan_tool_result_text` does; anything else degrades
/// to `None`. The id token is `[A-Za-z0-9_-]+`; the first `agentId: `
/// occurrence wins.
pub fn agent_id_from_tool_result_content(content: &serde_json::Value) -> Option<&str> {
    scan_tool_result_text(content, extract_agent_id_from_text)
}

/// Scan a plain text string for the first `agentId: <id>` token and return
/// the id (`[A-Za-z0-9_-]+`). Returns `None` if the marker is absent or no id
/// characters follow it.
fn extract_agent_id_from_text(text: &str) -> Option<&str> {
    const MARKER: &str = "agentId: ";
    let start = text.find(MARKER)? + MARKER.len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some(&rest[..end])
    }
}

/// Whether a tool_use launches the tool in the background — i.e. returns
/// immediately while the actual work continues, with a later
/// `<task-notification>` user line reporting its completion. The launching
/// tool_use `id` is recorded as the correlation key for that notification.
///
/// Modern Claude Code makes `Agent`/`Task` calls async by default and dropped
/// the `run_in_background` parameter from their schema, so for those tools the
/// absence of the key means background, not foreground. An explicit
/// `run_in_background: false` is still respected for forward compatibility, and
/// `true` is honoured for any tool (so a Bash invocation still has to opt in
/// explicitly — its default is foreground). Any other tool with no explicit
/// flag stays foreground.
pub fn launches_in_background(tool_name: &str, tool_use_input: &serde_json::Value) -> bool {
    let explicit = tool_use_input
        .get("run_in_background")
        .and_then(serde_json::Value::as_bool);
    match (tool_name, explicit) {
        (_, Some(b)) => b,
        ("Agent" | "Task", None) => true,
        _ => false,
    }
}

/// The tool names that spawn a subagent.
///
/// The current Claude Code build names this tool `Agent`; older builds named it
/// `Task`. Matched defensively against both so the hook contract / transcript
/// content can drift without breaking attribution.
pub const SUBAGENT_TOOL_NAMES: [&str; 2] = ["Agent", "Task"];

/// Whether `tool_name` names a subagent-spawning tool (see
/// [`SUBAGENT_TOOL_NAMES`]).
pub fn is_subagent_tool(tool_name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_is_extracted_from_an_array_of_text_blocks() {
        let content = serde_json::json!([
            {
                "type": "text",
                "text": "Async agent launched successfully.\n\
                         agentId: a6a7d31c908cdfa24 (internal ID - do not mention to user.)\n\
                         The agent is working in the background.",
            }
        ]);
        assert_eq!(
            agent_id_from_tool_result_content(&content),
            Some("a6a7d31c908cdfa24")
        );
    }

    #[test]
    fn agent_id_extraction_returns_none_when_marker_is_absent() {
        let content = serde_json::json!([
            { "type": "text", "text": "no marker here, just regular tool output." }
        ]);
        assert_eq!(agent_id_from_tool_result_content(&content), None);
    }

    #[test]
    fn agent_id_extraction_degrades_for_non_string_and_malformed_content() {
        // Not an array, object, or string — a bare number is unknown shape.
        assert_eq!(
            agent_id_from_tool_result_content(&serde_json::json!(42)),
            None
        );
        // Object missing the `text` field.
        assert_eq!(
            agent_id_from_tool_result_content(&serde_json::json!({ "type": "text" })),
            None
        );
    }

    #[test]
    fn launches_in_background_reads_the_top_level_flag() {
        assert!(launches_in_background(
            "Bash",
            &serde_json::json!({
                "command": "long-running",
                "run_in_background": true,
            }),
        ));
        assert!(!launches_in_background(
            "Bash",
            &serde_json::json!({
                "command": "ls",
                "run_in_background": false,
            }),
        ));
        // For a Bash call (or any non-subagent tool), absent key, wrong type,
        // and non-object inputs are all "foreground".
        assert!(!launches_in_background(
            "Bash",
            &serde_json::json!({ "command": "ls" }),
        ));
        assert!(!launches_in_background(
            "Bash",
            &serde_json::json!({ "run_in_background": "true" }),
        ));
        assert!(!launches_in_background("Bash", &serde_json::Value::Null));
    }

    #[test]
    fn launches_in_background_defaults_to_true_for_modern_agent_input() {
        // Modern Claude Code dropped the `run_in_background` parameter from the
        // `Agent`/`Task` tool schema and made these calls async by default. A
        // tool_use input that does not mention the key must be treated as
        // background, otherwise the running-subagent indicator clears as soon
        // as the immediate `PostToolUse(Agent)` fires.
        let modern_agent_input = serde_json::json!({
            "subagent_type": "general-purpose",
            "description": "Run ls and count entries",
            "prompt": "…",
        });
        assert!(launches_in_background("Agent", &modern_agent_input));
        assert!(launches_in_background("Task", &modern_agent_input));
        // A non-object input degrades to background for an Agent/Task call too:
        // the key is absent either way.
        assert!(launches_in_background("Agent", &serde_json::Value::Null));
    }

    #[test]
    fn launches_in_background_respects_explicit_false_for_agent() {
        // A caller that still passes `run_in_background: false` (e.g. an older
        // Claude Code, or a test fixture) keeps the foreground semantics it
        // asked for. Forward compatibility: if a future Claude reintroduces an
        // explicit foreground flag for Agent/Task, the predicate honours it.
        let explicit_foreground = serde_json::json!({
            "subagent_type": "general-purpose",
            "run_in_background": false,
        });
        assert!(!launches_in_background("Agent", &explicit_foreground));
        assert!(!launches_in_background("Task", &explicit_foreground));
        // An explicit `true` is honoured for Agent/Task as well, of course.
        assert!(launches_in_background(
            "Agent",
            &serde_json::json!({ "run_in_background": true }),
        ));
    }
}
