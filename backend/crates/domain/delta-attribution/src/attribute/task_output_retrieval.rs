//! The `TaskOutput` retrieval phase of the per-line fold: the parent's own
//! read of a background task's result, folded as that task's completion.

use delta_model::SessionId;

use crate::claude_format;

use super::{AttributionState, Effect};

/// Fold one `tool_result` as a possible `TaskOutput` retrieval: when it
/// reports that the retrieved background task has FINISHED, emit
/// [`Effect::SubagentCompleted`] for the launch it settles.
///
/// This is the second way a background subagent's running indicator clears.
/// The harness enqueues a `<task-notification>` only when it has to tell the
/// parent about a completion the parent did not ask for; when the parent
/// retrieves the result itself — `TaskOutput` with `block: true`, which waits
/// for the task — no notification is ever injected. Without this fold the
/// entry is never cleared: the turn-end sweep deliberately keeps background
/// entries, so the indicator spins forever and the persisted launch row keeps
/// re-seeding it.
///
/// The retrieval is recognized by its report's own bytes: the
/// `<retrieval_status>` element identifies the body as a retrieval report, and
/// the `<task_id>` it carries names the task it read. Correlating on the body
/// alone is what covers a BLOCKING retrieval, whose `tool_use` line is flushed
/// to the JSONL as soon as the assistant message completes while its
/// `tool_result` lands only when the task finishes — routinely a different
/// sync window, since the ambient tail polls throughout. Any in-memory
/// `tool_use` → `tool_result` correlation would be gone by then, so the report
/// is the single correlation path.
///
/// Thread attribution is deliberately untouched: the carrier line is a
/// `tool_result` and inherits `carry_thread` exactly as it does today. Only
/// the effects differ.
pub(super) fn resolve_task_output_retrieval(
    session_id: &SessionId,
    state: &mut AttributionState,
    effects: &mut Vec<Effect>,
    tool_use_id: &str,
    is_error: bool,
    content: &serde_json::Value,
) {
    if !claude_format::is_task_output_result(content) {
        // Not a retrieval at all — an ordinary tool's result.
        return;
    }
    if is_error {
        // The retrieval itself failed, so the task's state is unknown: leave
        // the running entry alone rather than clearing an indicator for a
        // subagent that may well still be working.
        return;
    }
    let Some(status) = claude_format::task_output_status(content) else {
        tracing::warn!(
            session_id = %session_id.as_str(),
            tool_use_id = %tool_use_id,
            "{} result carries no <status>; cannot tell whether the retrieved task \
             finished — the running indicator will not clear from this retrieval",
            claude_format::TASK_OUTPUT_TOOL_NAME,
        );
        return;
    };
    if !claude_format::is_terminal_task_status(status) {
        if status != claude_format::RUNNING_TASK_STATUS {
            tracing::warn!(
                session_id = %session_id.as_str(),
                tool_use_id = %tool_use_id,
                status = %status,
                "{} result carries an unrecognized <status>; treating it as still running, \
                 so the running indicator will not clear from this retrieval",
                claude_format::TASK_OUTPUT_TOOL_NAME,
            );
        }
        // A poll of a task that is still working: nothing to clear.
        return;
    }
    // The launch is keyed by the launching `tool_use` id, which a retrieval
    // never names — the `task_id` (the launch's `agentId`, learned via
    // `PostToolUse(Agent)`) is the only correlation key, exactly as for a
    // `<task-notification>` body that carries only `<task-id>`.
    let Some(task_id) = claude_format::task_output_task_id(content) else {
        tracing::warn!(
            session_id = %session_id.as_str(),
            tool_use_id = %tool_use_id,
            "{} result carries no <task_id> element; cannot match it against any \
             launched subagent",
            claude_format::TASK_OUTPUT_TOOL_NAME,
        );
        return;
    };
    // No recorded launch: the launch fell in an earlier window no longer
    // seeded into `launched_threads`, or its `task_id` was never learned.
    // Nothing to clear, and nothing surprising — the retrieval simply folds
    // as an ordinary `tool_result`.
    let Some(key) = state.launch_key_by_task_id(task_id) else {
        return;
    };
    if state.launched_threads.remove(&key).is_some() {
        // The same effect the notification path emits: it clears the
        // persisted launch row and finishes the running entry.
        effects.push(Effect::SubagentCompleted { tool_use_id: key });
    }
}
