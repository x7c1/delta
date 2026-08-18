//! The thread-resolution phase of the per-line fold: the thread a line is
//! attributed to, and the send / subagent effects that follow from it.

use delta_model::{MessageUuid, SessionId, ThreadId};

use crate::claude_format;

use super::{AttributionState, Effect};

/// Resolve the thread (and optional semantic parent) for one line, comparing it
/// against the head outstanding send and the recorded background launches.
/// Pushes the send/turn/subagent effects and mutates `state` (outstanding,
/// carry_thread, launched_threads) exactly as the inline `if`/`else` chain did.
///
/// `is_queued_command` is `line.is_queued_command` and `line_uuid` is
/// `line.uuid`; the classification flags are computed by the caller.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_line_thread(
    session_id: &SessionId,
    main_thread: ThreadId,
    state: &mut AttributionState,
    effects: &mut Vec<Effect>,
    line_uuid: &MessageUuid,
    trimmed: &str,
    is_queued_command: bool,
    is_local_command_name_line: bool,
    is_unknown_command_notice: bool,
    is_human_turn: bool,
    is_task_notification: bool,
) -> (ThreadId, Option<MessageUuid>) {
    // Compare against the head outstanding send; a match consumes it.
    if is_local_command_name_line {
        // The bare command-name line of a local-command group (e.g.
        // `/review-pr`). Delta dispatched it as a send and the turn machine
        // is `AwaitingEcho`, but a local command fires no `UserPromptSubmit`
        // echo and no `Stop` — so left alone the send wedges the queue
        // forever. When this line correlates to the head outstanding send,
        // treat it as a degenerate completed turn: consume the send
        // (`SendMatched`) and end the turn (`LocalCommandTurnEnded`, which the
        // caller feeds into the turn machine as a `Stop`). The correlation is
        // namespace-tolerant: Claude Code may record the command-name line in
        // its fully-qualified `/<namespace>:<command>` form (e.g.
        // `/example:review-pr`) even when the user — and thus the
        // dispatched send — used the short `/<command>` form (`/review-pr`), so
        // matching compares BARE command names rather than raw text. The line
        // is command machinery, so it inherits `carry_thread` and never resets
        // to `main`. (If it does NOT match an outstanding send — e.g. a
        // local command typed straight into the pane, never dispatched by
        // Delta — there is nothing to resolve; it simply folds as `Meta`.)
        let head_matches = state.outstanding.front().is_some_and(|send| {
            claude_format::local_command_name_line_matches_send(&send.text, trimmed)
        });
        if let Some(pending) = head_matches
            .then(|| state.outstanding.pop_front())
            .flatten()
        {
            effects.push(Effect::SendMatched {
                send_id: pending.id,
                matched_uuid: line_uuid.clone(),
            });
            effects.push(Effect::LocalCommandTurnEnded);
        }
        (state.carry_thread, None)
    } else if is_unknown_command_notice {
        // The unknown-command notice (e.g. the user typed `/revew-pr`). Delta
        // dispatched the command as a send and the turn machine is
        // `AwaitingEcho`, but Claude rejects an unknown command client-side —
        // no `UserPromptSubmit` echo, no `Stop`, and no command group — so
        // left alone the send wedges the queue forever, exactly like a known
        // local command. Treat it as a degenerate completed turn: when the
        // notice's command matches the head outstanding send, consume the send
        // (`SendMatched`) and end the turn (`LocalCommandTurnEnded`, which the
        // caller feeds into the turn machine as a `Stop`). Correlate robustly:
        // the send may carry args (`/review-pr 123`) while the notice names
        // only the command (`/review-pr`), so match the notice's command
        // against the send's FIRST whitespace-delimited token. The notice is
        // machinery, so it inherits `carry_thread` and never resets to `main`.
        // (If it matches no outstanding send — an unknown command typed
        // straight into the pane, never dispatched by Delta — there is nothing
        // to resolve; it simply surfaces as a `Role::System` notice.)
        let notice_command = claude_format::unknown_command_from_notice(trimmed);
        let head_matches = notice_command.is_some_and(|command| {
            state
                .outstanding
                .front()
                .is_some_and(|send| send.text.split_whitespace().next() == Some(command))
        });
        if let Some(pending) = head_matches
            .then(|| state.outstanding.pop_front())
            .flatten()
        {
            effects.push(Effect::SendMatched {
                send_id: pending.id,
                matched_uuid: line_uuid.clone(),
            });
            effects.push(Effect::LocalCommandTurnEnded);
        }
        (state.carry_thread, None)
    } else if is_human_turn {
        // Text-based correlation against the head outstanding send. Exact
        // equality for a plain send; `prompt_echoes_send` additionally
        // recognizes the rewrite Claude Code applies to an image-attachment
        // send, whose swallowed path line comes back as a leading `[Image #N]`
        // placeholder — a send exact equality alone could never match.
        let head_matches = state
            .outstanding
            .front()
            .is_some_and(|send| claude_format::prompt_echoes_send(&send.text, trimmed));
        match head_matches
            .then(|| state.outstanding.pop_front())
            .flatten()
        {
            Some(pending) => {
                effects.push(Effect::SendMatched {
                    send_id: pending.id,
                    matched_uuid: line_uuid.clone(),
                });
                state.carry_thread = pending.thread_id;
                (pending.thread_id, pending.semantic_parent_uuid)
            }
            None if is_queued_command => {
                // A LEGACY `queued_command` attachment with no matching
                // send is a programmatic injection, not stray pane typing,
                // so it must not tear the active turn back to `main` —
                // inherit the current thread the way a non-human line does.
                // (Harness-injected `<task-notification>` lines, which
                // current claude delivers as plain user lines rather than
                // `queued_command` attachments, are handled earlier: they
                // are excluded from `is_human_turn` and inherit
                // `carry_thread` via the `else` branch, like interrupt
                // markers and tool_result lines.)
                (state.carry_thread, None)
            }
            None => {
                state.carry_thread = main_thread;
                (main_thread, None)
            }
        }
    } else if is_task_notification {
        // A background task's completion: attribute it to the thread that
        // launched the task, not the thread that happens to be current now.
        // The notification carries two correlation keys — `<tool-use-id>`
        // and `<task-id>` — and Claude Code's user-message body sometimes
        // ships only one of them. Prefer `<tool-use-id>` (the existing key,
        // recorded at launch time); fall back to `<task-id>` (recorded
        // later via `PostToolUse(Agent)` for a tool launch, already at
        // launch for a forked skill). A match consumes the entry and
        // emits `SubagentCompleted` so the persisted correlation is
        // cleared. When neither key matches a recorded launch — the launch
        // fell in an earlier window no longer seeded into
        // `launched_threads`, or both elements were stripped from the body
        // — fall back to inheriting `carry_thread`, the prior no-regression
        // behaviour. A body carrying NEITHER element is logged so a future
        // Claude Code format change surfaces in the logs instead of as
        // stuck running indicators.
        let notification_tool_use_id = claude_format::task_notification_tool_use_id(trimmed);
        let notification_task_id = claude_format::task_notification_task_id(trimmed);
        if notification_tool_use_id.is_none() && notification_task_id.is_none() {
            tracing::warn!(
                session_id = %session_id.as_str(),
                thread_id = state.carry_thread.value(),
                "<task-notification> body carries no <tool-use-id> nor <task-id>; \
                 cannot match against any launched subagent — the running indicator \
                 will not clear from this notification"
            );
        }
        let by_tool_use_id = notification_tool_use_id
            .filter(|id| state.launched_threads.contains_key(*id))
            .map(str::to_owned);
        let resolved = by_tool_use_id.or_else(|| {
            let task_id = notification_task_id?;
            state
                .launched_threads
                .iter()
                .find(|(_, launch)| launch.task_id.as_deref() == Some(task_id))
                .map(|(tool_use_id, _)| tool_use_id.clone())
        });
        match resolved.and_then(|key| {
            state
                .launched_threads
                .remove(&key)
                .map(|launch| (key, launch))
        }) {
            Some((tool_use_id, launch)) => {
                effects.push(Effect::SubagentCompleted { tool_use_id });
                // Advance the turn onto the launching thread: the
                // assistant's continuation of this notification belongs to
                // the task's thread, not the thread that was current when
                // the completion happened to land.
                state.carry_thread = launch.thread_id;
                (launch.thread_id, None)
            }
            None => (state.carry_thread, None),
        }
    } else {
        (state.carry_thread, None)
    }
}
