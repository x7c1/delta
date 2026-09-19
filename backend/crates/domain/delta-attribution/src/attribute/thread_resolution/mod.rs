//! The thread-resolution phase of the per-line fold: the thread a line is
//! attributed to, and the send / subagent effects that follow from it.
//!
//! The `if`/`else` cascade over a line's classification lives here; the one
//! branch whose correlation rules stand on their own — the
//! `<task-notification>` completion signal — lives in [`task_notification`].

use delta_model::{MessageUuid, SessionId, ThreadId};

use crate::claude_format;

use super::{AttributionState, Effect};

mod task_notification;
use task_notification::resolve_task_notification;

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
    // Correlate against the head outstanding send: a human turn consumes it by
    // POSITION; a command line does too, but only when that send is itself a
    // slash command (see `consume_slash_command_send`).
    if is_local_command_name_line {
        // The bare command-name line of a local-command group (e.g.
        // `/review-pr`). Delta dispatched it as a send and the turn machine
        // is `AwaitingEcho`, but a local command fires no `UserPromptSubmit`
        // echo and no `Stop` — so left alone the send wedges the queue
        // forever. This line is therefore the outstanding slash command's
        // outcome: consume the send (`SendMatched`) and end the degenerate
        // turn (`LocalCommandTurnEnded`, which the caller feeds into the turn
        // machine as that send's own resolution).
        //
        // The recorded command NAME does not decide that. Claude Code may
        // write the line in its fully-qualified `/<namespace>:<command>` form
        // (e.g. `/example:review-pr`) even though the user — and thus the
        // send — used the short `/<command>` form, and that is only the
        // rewrite already catalogued. So the name comparison is demoted to
        // `attributed`.
        //
        // The line is command machinery, so it inherits `carry_thread` and
        // never resets to `main`. (With no outstanding slash-command send —
        // a local command typed straight into the pane, or a plain prompt
        // outstanding that this line cannot be the echo of — there is nothing
        // to resolve; it simply folds as `Meta`.)
        consume_slash_command_send(state, effects, line_uuid, |send_text| {
            claude_format::local_command_name_line_matches_send(send_text, trimmed)
        });
        (state.carry_thread, None)
    } else if is_unknown_command_notice {
        // The unknown-command notice (e.g. the user typed `/revew-pr`). Delta
        // dispatched the command as a send and the turn machine is
        // `AwaitingEcho`, but Claude rejects an unknown command client-side —
        // no `UserPromptSubmit` echo, no `Stop`, and no command group — so
        // left alone the send wedges the queue forever, exactly like a known
        // local command. Same rule as the branch above: the notice is the
        // outstanding slash command's outcome, so consume the send
        // (`SendMatched`) and end the degenerate turn
        // (`LocalCommandTurnEnded`).
        //
        // The name is a report here too, and a weak one: the notice names
        // whatever Claude parsed out of a command it did not recognize, which
        // for a typo is exactly the name that will not equal the send's. So
        // `attributed` compares the notice's command against the send's FIRST
        // whitespace-delimited token — tolerating args the send carries
        // (`/review-pr 123`) that the notice drops — and a mismatch is
        // logged, not acted on. A notice naming no command at all reports
        // `false` for the same reason.
        //
        // The notice is machinery, so it inherits `carry_thread` and never
        // resets to `main`. (With no outstanding slash-command send it simply
        // surfaces as a `Role::System` notice.)
        let notice_command = claude_format::unknown_command_from_notice(trimmed);
        consume_slash_command_send(state, effects, line_uuid, |send_text| {
            notice_command
                .is_some_and(|command| send_text.split_whitespace().next() == Some(command))
        });
        (state.carry_thread, None)
    } else if is_human_turn {
        // POSITIONAL correlation against the head outstanding send, mirroring
        // the rule the turn machine already consumes a send by (see
        // `on_user_prompt_submit`): under the single-outstanding dispatch rule
        // at most one send is outstanding and its keystrokes are already in the
        // pane, so the first human user line to arrive while it is outstanding
        // IS its echo — whatever the text turned out to be. Claude Code rewrites
        // a prompt freely between the keystrokes landing and the transcript
        // being written (the `[Image #N]` attachment placeholder, the
        // `<pasted_content>` wrapper around a pasted body, and shapes we have
        // not catalogued yet), and a prompt genuinely typed into the pane in
        // that window was credited to the send by the turn machine anyway.
        // Deciding by text here would file the send's own user line — and the
        // whole reply that follows it — on `main`, so from the send's thread the
        // turn simply vanishes.
        //
        // The text comparison keeps exactly one job: `prompt_echoes_send` (exact
        // equality for a plain send, widened to absorb the image-attachment
        // rewrite and the pasted-content wrapper) says whether the echo is
        // recognizable as the send's text, and that verdict rides along as
        // `SendMatched::attributed` so a new rewrite shape is visible in the log
        // the first time it happens. It gates nothing. `trimmed` is the line as
        // Claude Code submitted it — the fold unwraps the pasted-content wrapper
        // only for the stored message — so this verdict and the
        // `UserPromptSubmit` path's are the same rule over the same text.
        match state.outstanding.pop_front() {
            Some(pending) => {
                effects.push(Effect::SendMatched {
                    send_id: pending.id,
                    matched_uuid: line_uuid.clone(),
                    attributed: claude_format::prompt_echoes_send(&pending.text, trimmed),
                });
                state.carry_thread = pending.thread_id;
                (pending.thread_id, pending.semantic_parent_uuid)
            }
            None if is_queued_command => {
                // A LEGACY `queued_command` attachment arriving with NO
                // outstanding send is a programmatic injection, not stray pane
                // typing, so it must not tear the active turn back to `main` —
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
                // No send is outstanding, so nothing Delta dispatched can
                // explain this line: it is input typed straight into the pane.
                // It starts a new turn on `main` and resets the carry.
                state.carry_thread = main_thread;
                (main_thread, None)
            }
        }
    } else if is_task_notification {
        // A background task's completion. `resolve_task_notification` owns the
        // correlation rules (which recorded launch the notification reports)
        // and the effects that follow; the line never names a semantic parent.
        (
            resolve_task_notification(session_id, state, effects, trimmed),
            None,
        )
    } else {
        (state.carry_thread, None)
    }
}

/// Consume the head outstanding send against a command line — the
/// local-command name line or the unknown-command notice — and end the
/// degenerate turn it stood for.
///
/// The correlation is POSITIONAL but guarded by KIND: the head send is
/// consumed when it is itself a slash command
/// ([`claude_format::is_slash_command_send`]), whatever command name the line
/// ended up recording. That guard is not optional. A slash command produces
/// no `UserPromptSubmit` echo, so nothing has consumed the send yet and this
/// line is the only evidence it was submitted; a PLAIN-prompt send, by
/// contrast, is consumed by the hook the moment it is submitted, so a command
/// line arriving while one is outstanding means something else was typed into
/// the pane — consuming the send there would drop the user's message on the
/// floor. Such a send is left outstanding for its own echo (or, failing that,
/// for the echo deadline to requeue it).
///
/// `attributed` reports whether the line names the send's own command; it
/// gates nothing, and rides out on `Effect::SendMatched` so a name Delta
/// cannot account for is visible in the log.
fn consume_slash_command_send(
    state: &mut AttributionState,
    effects: &mut Vec<Effect>,
    line_uuid: &MessageUuid,
    attributed: impl FnOnce(&str) -> bool,
) {
    let head_is_slash_command = state
        .outstanding
        .front()
        .is_some_and(|send| claude_format::is_slash_command_send(&send.text));
    if !head_is_slash_command {
        return;
    }
    let Some(pending) = state.outstanding.pop_front() else {
        return;
    };
    effects.push(Effect::SendMatched {
        send_id: pending.id,
        matched_uuid: line_uuid.clone(),
        attributed: attributed(&pending.text),
    });
    effects.push(Effect::LocalCommandTurnEnded {
        send_id: pending.id,
    });
}
