---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "TODO: clean leaked entries" backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/mod.rs && grep -q "pending_post_tool_use_agent_ids.clear()" backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/subagents.rs && grep -q "pending_post_tool_use_agent_ids.clear()" backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/turn.rs'
assignee: null
branch: task/0827-2313-fix-clear-pending-agent-ids-when-the-process-is-gone
created_at: 2026-08-27T14:13:46Z
updated_at: 2026-08-27T16:46:15Z
---

# fix(session-actor): clear the pending `PostToolUse` agent-id buffer once the agent process is gone

## Overview

`SessionRuntime::pending_post_tool_use_agent_ids`
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/mod.rs`,
the field doc around lines 146-176) buffers the `agentId` that
`PostToolUse(Agent)` reports for a background launch whose running entry does
not exist yet, keyed by `tool_use_id`, so that the `Effect::SubagentIndicatorStarted`
arm of `sync_transcript` (`interactor/sync/sync_transcript.rs`, the drain near
line 320) can attach it when the parent transcript finally folds the launch
line. The only two accessors are `record_pending_post_tool_use_agent_id` and
`drain_pending_post_tool_use_agent_id` in `runtime/subagents.rs`; the only
insert site is `interactor/hooks/on_post_tool_use.rs` (unconditional whenever
`tool_response.agentId` parses non-empty).

The field carries a `TODO: clean leaked entries on session lifecycle events`:
a nested `Agent` launch (a subagent calling `Agent` itself) also reaches the
hook, but its `tool_use_id` lives in the subagent's own JSONL and never appears
in the parent's, so its entry is never drained. It cannot be filtered at insert
time — the wire payload (`delta-wire/src/hooks/post_tool_use_payload.rs`) carries
no parent/depth signal, and the existing `is_foreign_transcript` guard
(`hooks/hook_transcript_guard.rs`) does not catch it because Claude Code
2.1.193 presents a nested launch's hook with the *parent's* `transcript_path`
(see the regression notes at the top of
`hooks/tests/nested_subagent_hook_is_filtered.rs`). Today the leak is reclaimed
only when the actor retires.

Close the TODO with the smallest honest sweep: clear the buffer at the two
places where the agent process is known to be gone, so no matching `tool_use`
line can ever be folded afterwards.

- `SessionRuntime::drain_running_subagents` (`runtime/subagents.rs`, the
  `std::mem::take` of `running_subagents`) — called from the session-close and
  session-end sweeps (`lifecycle/close_session.rs`, `hooks/on_session_end.rs`
  → `sweep_running_subagents.rs`). The close path syncs the transcript right
  before sweeping, so a legitimately pending id has already had its drain; the
  session-end path does not sync, but once the process is gone no
  `<task-notification>` for a not-yet-folded launch can arrive, so an id
  dropped there has nothing left to correlate with. Add
  `self.pending_post_tool_use_agent_ids.clear()` there and extend the method
  doc to say the pending buffer goes with the running set.
- `SessionRuntime::forget_turn` (`runtime/turn.rs`) — the session-deletion
  reset that already clears `running_subagents` for the same reason. Add the
  same `clear()` and mention it in the doc.

Do **not** clear at turn end (`runtime/turn.rs`, the `next == Idle` branch of
`apply_turn`): the buffer exists precisely because the launch line may not be
in the parent JSONL yet when the turn-end hook fires, and only some paths into
`Idle` sync before applying the turn. Do not try to evict on
`Effect::SubagentCompleted` either — a nested launch's completion notification
never reaches the parent fold, so that arm would only ever remove ids the
`SubagentIndicatorStarted` drain already took.

Then rewrite the field doc in `runtime/mod.rs`: remove the TODO paragraph and
replace it with a short statement of (1) why a nested launch's entry cannot be
told apart at insert time, and (2) where the buffer is now cleared (the two
methods above), keeping the existing explanation of the race the buffer solves.
The doc must no longer say the entries are reclaimed only by actor retirement.

### Tests

The buffer has no accessor on `SessionLiveState`, so pin the behaviour at the
runtime level in the existing `mod tests` at the bottom of
`runtime/subagents.rs` (next to the `drain_running_subagents` tests): record a
pending id, call `drain_running_subagents`, and assert that
`drain_pending_post_tool_use_agent_id` for that `tool_use_id` now returns
`None`; a sibling test does the same through `forget_turn`. Name them in the
style of their neighbours. Keep
`sync/tests/post_tool_use_arriving_before_the_launch_is_folded_persists_the_agent_id.rs`
green — it is the test for the drain the buffer exists for, and this task must
not weaken it.

Run `make check` and fix whatever it reports. The frontend is untouched.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `drain_running_subagents` and `forget_turn` both clear
      `pending_post_tool_use_agent_ids` (gates appended), and a runtime-level
      unit test per method asserts a previously recorded id is no longer
      drainable afterwards.
- [x] The `TODO: clean leaked entries` paragraph is gone from
      `runtime/mod.rs` (gate appended) and the field doc names the two clear
      sites and why insert-time filtering is not possible.
- [x] `post_tool_use_arriving_before_the_launch_is_folded_persists_the_agent_id`
      still passes (`make check`).

## Out of scope

- Clearing the buffer at turn end, or any change to when the indicator
  lights / clears.
- Changing the hook payload or the `is_foreign_transcript` guard to detect
  nested launches.
