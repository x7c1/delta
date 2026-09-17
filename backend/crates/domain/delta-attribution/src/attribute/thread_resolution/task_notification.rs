//! The `<task-notification>` branch of thread resolution: which recorded
//! background launch a harness-injected completion notice reports, and the
//! effects that follow from matching it.

use delta_model::{SessionId, ThreadId};

use crate::attribute::{AttributionState, Effect};
use crate::claude_format;

/// A background task's completion: attribute it to the thread that launched
/// the task, not the thread that happens to be current now. Returns the
/// thread the notification line folds to.
///
/// Three outcomes:
///
/// 1. KEYED MATCH. The notification carries two correlation keys —
///    `<tool-use-id>` and `<task-id>` — and Claude Code's user-message body
///    sometimes ships only one of them. Prefer `<tool-use-id>` (the existing
///    key, recorded at launch time); fall back to `<task-id>` (recorded later
///    via `PostToolUse(Agent)` for a tool launch, already at launch for a
///    forked skill).
/// 2. KEYLESS SINGLE MATCH. A body carrying NEITHER element names no launch
///    at all, but with exactly ONE launch outstanding there is nothing to be
///    ambiguous about: the notification reports that one. It resolves to it
///    and takes the same path a keyed match takes.
/// 3. NO MATCH, reached two ways. A key that names no recorded launch — its
///    launch fell in an earlier window no longer seeded into
///    `launched_threads` — resolves to nothing even when a launch happens to
///    be outstanding: the key is evidence the notification belongs to some
///    OTHER launch, so it must not consume the one at hand. A keyless body
///    meeting zero or several outstanding launches likewise consumes nothing
///    — guessing among several would clear the wrong indicator and
///    misattribute the continuation, which is worse than a stale indicator.
///    Either way the line inherits `carry_thread`, the prior no-regression
///    behaviour.
///
/// A match consumes the entry and emits `SubagentCompleted` so the persisted
/// correlation is cleared.
pub(super) fn resolve_task_notification(
    session_id: &SessionId,
    state: &mut AttributionState,
    effects: &mut Vec<Effect>,
    trimmed: &str,
) -> ThreadId {
    let notification_tool_use_id = claude_format::task_notification_tool_use_id(trimmed);
    let notification_task_id = claude_format::task_notification_task_id(trimmed);
    let resolved = if notification_tool_use_id.is_none() && notification_task_id.is_none() {
        resolve_keyless_body(session_id, state)
    } else {
        notification_tool_use_id
            .filter(|id| state.launched_threads.contains_key(*id))
            .map(str::to_owned)
            .or_else(|| state.launch_key_by_task_id(notification_task_id?))
    };
    match resolved.and_then(|key| {
        state
            .launched_threads
            .remove(&key)
            .map(|launch| (key, launch))
    }) {
        Some((tool_use_id, launch)) => {
            effects.push(Effect::SubagentCompleted { tool_use_id });
            // Advance the turn onto the launching thread: the assistant's
            // continuation of this notification belongs to the task's thread,
            // not the thread that was current when the completion happened to
            // land.
            state.carry_thread = launch.thread_id;
            launch.thread_id
        }
        None => state.carry_thread,
    }
}

/// The launch key a keyless body — one carrying neither `<tool-use-id>` nor
/// `<task-id>` — resolves to: the only outstanding launch when there is
/// exactly one (outcome 2 above), and nothing otherwise (outcome 3).
///
/// Logged either way, matched or not, because the shape signals an upstream
/// format change — and once matched it leaves no other trace, so the log is
/// the only place the new body shape surfaces.
fn resolve_keyless_body(session_id: &SessionId, state: &AttributionState) -> Option<String> {
    let only_outstanding = match state.launched_threads.len() {
        1 => state.launched_threads.keys().next().cloned(),
        _ => None,
    };
    match &only_outstanding {
        Some(tool_use_id) => tracing::warn!(
            session_id = %session_id.as_str(),
            thread_id = state.carry_thread.value(),
            tool_use_id = %tool_use_id,
            "<task-notification> body carries no <tool-use-id> nor <task-id>; \
             matched it to the only outstanding background launch"
        ),
        None => tracing::warn!(
            session_id = %session_id.as_str(),
            thread_id = state.carry_thread.value(),
            outstanding_launches = state.launched_threads.len(),
            "<task-notification> body carries no <tool-use-id> nor <task-id> and the \
             outstanding launches are not exactly one, so it cannot be matched \
             against any launched subagent"
        ),
    }
    only_outstanding
}
