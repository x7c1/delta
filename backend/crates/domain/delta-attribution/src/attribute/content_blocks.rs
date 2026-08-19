//! The content-block phase of the per-line fold: resolve permissions from
//! `tool_result` blocks, record background-launch correlations, and light the
//! running-subagent indicator.

use delta_model::ContentBlock;

use crate::claude_format;

use super::{AttributionState, Effect, SubagentLaunch};

/// Process one line's content blocks. Pushes effects and mutates
/// `state.launched_threads` exactly as the inline loop did; reads
/// `state.carry_thread` as the launching thread. Returns nothing.
pub(super) fn process_content_blocks(
    state: &mut AttributionState,
    effects: &mut Vec<Effect>,
    content: &[ContentBlock],
) {
    // Correlate any tool_result blocks on this line with the open
    // permission requests they settle. Resolving on actual completion
    // (rather than at `PreToolUse` time) is what lets an auto-approved
    // tool's notice clear immediately while a genuine prompt's notice
    // persists until the human answers. A denied tool yields
    // `is_error: true` ("User rejected tool use"), so the error flag
    // infers allowed vs denied.
    for block in content {
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                is_error,
                content,
            } => {
                effects.push(Effect::ResolvePermission {
                    tool_use_id: tool_use_id.clone(),
                    allowed: !is_error,
                });
                if *is_error {
                    // A denied/errored LAUNCH: the `tool_result` reports the
                    // background `Agent`/`Task` never actually started (e.g.
                    // `toolDenialKind: "automode-blocked"`), so the
                    // `<task-notification>` that normally completes a
                    // background subagent will NEVER arrive. Left alone, the
                    // running indicator lit by the launch's `tool_use` stays
                    // stuck forever: the turn-end sweep deliberately KEEPS
                    // background entries, and reloading re-seeds from the
                    // same authoritative `running_subagents`.
                    //
                    // Gate on a recorded launch (`launched_threads`) rather
                    // than firing on every `is_error`. Only background
                    // `Agent`/`Task`/`Bash` launches are recorded here (see
                    // the `ToolUse` arm below), so this precisely targets the
                    // stuck background case: foreground subagent denials are
                    // never recorded and are already cleared by the turn-end
                    // sweep, and unrelated errored tools never get a spurious
                    // `SubagentCompleted`. `remove` drops the entry (mirroring
                    // the `<task-notification>` path) so a later stray
                    // notification can't double-fire and replay stays
                    // consistent. Reusing `SubagentCompleted` is intentional:
                    // its sync handler clears the stale persisted launch row
                    // and finishes the (id-keyed, kind-agnostic) running entry.
                    if state.launched_threads.remove(tool_use_id).is_some() {
                        effects.push(Effect::SubagentCompleted {
                            tool_use_id: tool_use_id.clone(),
                        });
                    }
                } else if let Some(launch) = state.launched_threads.get_mut(tool_use_id) {
                    // In-memory state recovery only — no `Effect` is emitted.
                    // The live `PostToolUse(Agent)` hook is responsible for
                    // persisting the `agentId` on the launch row; this branch
                    // mirrors that upgrade against the in-memory launch entry
                    // so a fold without the hook (cold-start replay / re-fold)
                    // can still match a `<task-notification>` body that ships
                    // only `<task-id>`. The structural sibling
                    // `toolUseResult.agentId` is not preserved in
                    // `ContentBlock::ToolResult`, so the id is rescued from the
                    // human-readable `tool_result` text instead.
                    if launch.task_id.is_none() {
                        if let Some(id) = claude_format::agent_id_from_tool_result_content(content)
                        {
                            launch.task_id = Some(id.to_owned());
                        }
                    }
                }
            }
            // A background `Agent`/`Task`/`Bash` (async-by-default for
            // Agent/Task, opt-in `run_in_background: true` for Bash) returns
            // immediately; its completion is injected later as a
            // `<task-notification>` carrying this same `id`. Record
            // `(tool_use_id -> launching thread)` so that notification —
            // possibly in a later sync window — is attributed to the thread
            // that launched it rather than whatever thread is current then.
            // The launching thread is `carry_thread`: a tool_use is part of
            // the in-flight turn, whose thread `carry_thread` already holds.
            ContentBlock::ToolUse { id, name, input }
                if claude_format::launches_in_background(name, input) =>
            {
                // Re-folding a launch line refreshes the launching thread.
                // If the same id was already seeded from the persisted
                // store with a `task_id` upgrade, preserve it — it was
                // learned later via `PostToolUse(Agent)` and a fresh fold
                // of the launch line itself has no newer information.
                // Mirrors the SQL `record_subagent_launch` UPSERT, which
                // only touches the `thread_id` column.
                let task_id = state
                    .launched_threads
                    .get(id)
                    .and_then(|launch| launch.task_id.clone());
                state.launched_threads.insert(
                    id.clone(),
                    SubagentLaunch {
                        thread_id: state.carry_thread,
                        task_id,
                    },
                );
                effects.push(Effect::SubagentLaunched {
                    tool_use_id: id.clone(),
                    thread_id: state.carry_thread,
                    // Never known at launch — it arrives with the launching
                    // tool's `tool_result` and upgrades the launch row then.
                    // Unlike the `launched_threads` entry above, which can
                    // carry an id an earlier fold already learned, the effect
                    // reports only what THIS line taught.
                    task_id: None,
                });
            }
            _ => {}
        }

        // The running-subagent indicator is driven from this parent-side
        // transcript ingest — NOT from the `PreToolUse` hook. Every
        // `Agent`/`Task` tool_use written to the parent's JSONL lights the
        // indicator (foreground OR background), and is cleared later by the
        // matching `PostToolUse` (foreground) or `<task-notification>`
        // (background). A NESTED subagent's `Agent`/`Task` tool_use is
        // written to the SUBAGENT's JSONL, never the parent's, so this
        // branch is the natural filter: nested launches never produce a
        // parent indicator and can never get stuck.
        if let ContentBlock::ToolUse { id, name, input } = block {
            if claude_format::is_subagent_tool(name) {
                let subagent_type =
                    claude_format::json_string_field(input, "subagent_type").map(str::to_owned);
                let description =
                    claude_format::json_string_field(input, "description").map(str::to_owned);
                let background = claude_format::launches_in_background(name, input);
                effects.push(Effect::SubagentIndicatorStarted {
                    tool_use_id: id.clone(),
                    thread_id: state.carry_thread,
                    subagent_type,
                    description,
                    background,
                });
            }
        }
    }
}
