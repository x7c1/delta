//! The forked-skill phase of the per-line fold: the launch of a background
//! agent that the CLI harness — not the model — started.
//!
//! A slash command whose skill runs in the background (e.g. `/review-pr`,
//! recorded as `/example:review-pr`) is launched by Claude Code itself, so
//! the parent transcript carries **no `tool_use` block** for it: the only trace
//! is a `<forked-skill-launch>` element on the command's
//! `type: "system"` / `subtype: "local_command"` line. Without folding that
//! element nothing lights the running-subagent indicator, and — because the
//! same group is folded as a degenerate, already-finished turn (see
//! [`Effect::LocalCommandTurnEnded`]) — the session sits completely inert for
//! the minutes the skill works.

use delta_model::{Role, SessionId};

use crate::claude_format;

use super::{AttributionState, Effect, SubagentLaunch};

/// Fold the `<forked-skill-launch>` element a line may carry: record the launch
/// and light a background running indicator for it.
///
/// Within a harness-written line, detection is by content alone, deliberately
/// independent of the local-command `promptId` grouping: the line carrying this
/// element has no `promptId` at all, so it is not a member of that group.
///
/// `role` is the line's PARSED role, and only [`Role::Meta`] — the role the
/// gateway parser folds a `subtype: "local_command"` system line to — is
/// considered. The gate matters because the payload is plain text a human or the
/// model can also write: a prompt or an assistant message that merely QUOTES a
/// `<forked-skill-launch>` element (Delta's own transcripts, task docs and tests
/// contain one verbatim, and Delta is developed inside Delta) would otherwise
/// mint a background launch for an agent that does not exist — and, being
/// background, its indicator would survive every turn end and stay lit until the
/// session closed, since no `<task-notification>` can ever arrive for it. Every
/// other content-based classifier in the fold is role-gated the same way (the
/// interrupt marker and `<task-notification>` require [`Role::User`]).
///
/// The launching thread is `state.carry_thread` — the same thread the group's
/// lines and the later `<task-notification>` are attributed to, so the
/// indicator, the messages and the unread suppression all agree.
///
/// An element whose body is unusable (unparsable, or naming no `agentId`) is
/// logged and otherwise ignored: the `agentId` is the correlation key for the
/// whole lifecycle, so a launch minted without one could never be completed.
pub(super) fn process_forked_skill_launch(
    session_id: &SessionId,
    state: &mut AttributionState,
    effects: &mut Vec<Effect>,
    role: Role,
    trimmed_text: &str,
) {
    if !matches!(role, Role::Meta) || !claude_format::has_forked_skill_launch(trimmed_text) {
        return;
    }
    let Some(launch) = claude_format::forked_skill_launch(trimmed_text) else {
        tracing::warn!(
            session_id = %session_id.as_str(),
            thread_id = state.carry_thread.value(),
            "<forked-skill-launch> element carries no usable payload (no parsable \
             JSON body naming an agentId) — the running indicator will not light \
             for this forked skill"
        );
        return;
    };

    // A forked skill has no `tool_use` id, so it is tracked under a synthetic
    // one derived from its `agentId` (namespaced so it can never collide with a
    // genuine `toolu_...`).
    let tool_use_id = launch.tool_use_id();
    // Unlike a tool-launched background subagent — which learns its `agentId`
    // later, from the launching tool's `tool_result` — this launch knows the
    // background-task id up front: the payload's `agentId` IS the `<task-id>`
    // the completion notification carries. Seed it into the launch map as well
    // as the effect, so a notification folded in the SAME window matches too.
    state.launched_threads.insert(
        tool_use_id.clone(),
        SubagentLaunch {
            thread_id: state.carry_thread,
            task_id: Some(launch.agent_id.clone()),
        },
    );
    effects.push(Effect::SubagentLaunched {
        tool_use_id: tool_use_id.clone(),
        thread_id: state.carry_thread,
        task_id: Some(launch.agent_id),
    });
    // Background, always: a forked skill returns immediately and keeps working
    // long after the launching (degenerate) turn ends, so the entry must
    // survive the turn-end sweep and be finished only by its
    // `<task-notification>`.
    effects.push(Effect::SubagentIndicatorStarted {
        tool_use_id,
        thread_id: state.carry_thread,
        subagent_type: launch.skill_name,
        description: launch.description,
        background: true,
    });
}
