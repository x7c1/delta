---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && test -f frontend/packages/apps/web/e2e-fake/subagent-running-task-output.spec.ts && grep -rq "TaskOutput" backend/crates/domain/delta-attribution/src/'
assignee: null
branch: task/0901-0457-fix-clear-background-subagent-on-task-output-retrieval
created_at: 2026-09-01T04:57:49Z
updated_at: 2026-09-01T07:18:40Z
---

# fix(attribution): clear a background subagent's running indicator when its result is retrieved via `TaskOutput`

## Overview

A background subagent's running entry is cleared only when its completion
`<task-notification>` is folded from the parent transcript
(`backend/crates/domain/delta-attribution/src/attribute/thread_resolution.rs`,
the `is_task_notification` arm around lines 141-196, emitting
`Effect::SubagentCompleted`). But Claude Code does **not** enqueue a
`<task-notification>` when the parent retrieves the task's result itself via a
blocking `TaskOutput` tool call before the notification fires. In that flow the
parent JSONL carries only an assistant `tool_use` named `TaskOutput` (input:
`{"task_id": "<agentId>", "block": true, ...}`) and a successful `tool_result`
whose text body contains `<retrieval_status>success</retrieval_status>` and
`<status>completed</status>`. Nothing in Delta recognizes that pair — `TaskOutput`
is not in `SUBAGENT_TOOL_NAMES` (`claude_format/mod.rs:438`), and the only
`tool_result`-driven completion is the `is_error` branch of
`attribute/content_blocks.rs` (lines 37-65), keyed on the result's own
`tool_use_id`, which for a `TaskOutput` call is not the launch's id. The entry
therefore leaks in `SessionRuntime::running_subagents` forever: the turn-end
sweep deliberately keeps `background: true` entries (`runtime/turn.rs`, the
`retain` in the `Idle` transition), no periodic sweep inspects the set, the
navigator row spins indefinitely (`SessionNode.tsx`, `data-testid="session-running"`),
the leaked entry keeps re-seeding the frontend on every focus
(`usePendingSends.ts` → `seedRunningSubagents` replaces), and it pins the actor
alive (`runtime/mod.rs`, `is_empty()` includes `running_subagents.is_empty()`).
Observed in production dogfooding 2026-08-30: an Explore agent launched with
`run_in_background: true`, consumed via `TaskOutput(block: true)`, still listed
in `GET /api/sessions/{id}/sends` `running_subagents` two days later with the
turn `idle`.

### Fix 1 — fold `TaskOutput` retrievals as completions (the main fix)

In `delta-attribution`:

- `claude_format`: add parsers for the retrieval report a `TaskOutput` call
  writes as its `tool_result` body — detect the report by its
  `<retrieval_status>` element, and read its `<status>…</status>` and
  `<task_id>…</task_id>` elements — next to the existing
  `task_notification_*` parsers. Do **not** add `TaskOutput` to
  `SUBAGENT_TOOL_NAMES` — it is a retrieval, not a launch.
- When folding a `ToolResult` whose body is a retrieval report, and only if
  the result is not an error and its `<status>` is terminal (`completed`,
  `failed`, or `killed`), resolve the report's `<task_id>` against
  `state.launched_threads` exactly like the `<task-id>` fallback at
  `thread_resolution.rs:172-179` (`launch.task_id` is populated from
  `PostToolUse(Agent)`), `remove` the launch, and emit
  `Effect::SubagentCompleted { tool_use_id }` — the same effect the
  notification path emits, so `finish_subagent` and the `SubagentFinished`
  broadcast need no changes. The report body is the correlation source, not
  the `TaskOutput` call's own `tool_use` input: a blocking retrieval's
  `tool_use` line is flushed to the transcript long before its `tool_result`,
  so the two fold in different sync windows and any same-window pairing would
  never fire in production (the report carries `<task_id>` in every observed
  real transcript). A `<status>running</status>` poll (non-blocking
  `TaskOutput`), a missing/unknown `<status>` (log a `tracing::warn!` in the
  style of the no-key notification warning at `thread_resolution.rs:160-168`),
  an errored result, or a `task_id` matching no recorded launch must all leave
  the running entry untouched. Do not change `carry_thread` / thread
  attribution from this path — a `tool_result` carrier line attributes as it
  does today; only the effects differ.

### Fix 2 — sweep background entries on a failed resume's `SessionEnd`

`hooks/on_session_end.rs`: the failed-resume branch (lines 77-99) kills the
pane and feeds `TurnInput::Close`, but returns without calling
`sweep_running_subagents_on_process_gone()` — asymmetric with the normal-end
path (line 118), whose comment explains why the sweep is needed once the
process is gone. A background entry from a turn before the resume window leaks
the same way. Call the sweep in that branch too and append its
`SubagentFinished` events to the returned `SpawnFailed` event, mirroring the
normal-end path.

### fake-claude + e2e-fake coverage

`backend/crates/apps/fake-claude/src/scenario.rs` (and its executor in
`run.rs` / `transcript.rs`): add a step `task_output { status? }` (default
`"completed"`) that models the parent retrieving the most recent background
`tool_use`'s result: write the assistant `tool_use` line named `TaskOutput`
whose input carries the minted `agentId` as `task_id` (the id
`task_notification` already uses) and `block: true`, fire
`PreToolUse`/`PostToolUse` like the existing `tool_use`/`post_tool_use` steps,
and write the successful `tool_result` line whose text body carries
`<retrieval_status>success</retrieval_status>`, `<task_id>…</task_id>`, and
`<status>{status}</status>` — the real bytes Claude Code produces. Document it
in the module-docs vocabulary table.

Add `frontend/packages/apps/web/e2e-fake/scenarios/subagent-running-task-output.json`
mirroring `subagent-running-background-task-id.json`, with the
`task_notification` step replaced by `task_output`, and a spec
`subagent-running-task-output.spec.ts` mirroring
`subagent-running-background-task-id.spec.ts`: the navigator spinner
(`session-running`) is visible after the launching turn stops and disappears
after the retrieval turn.

### Tests (usecase level)

In `backend/crates/domain/delta-usecase/src/interactor/`:

- `sync/tests/background_task_output_retrieval_clears_the_running_subagent.rs`
  — mirror `background_task_completion_clears_the_running_subagent.rs`, but
  Window 2 folds a `TaskOutput` `tool_use` + successful `completed` result
  instead of the notification; assert `SubagentFinished` is broadcast and
  `running_subagents` empties. Establish the `task_id` correlation the same
  way the existing task-id-fallback test does (via `PostToolUse(Agent)`
  reporting the `agentId`). Add helpers `task_output_tool_use_line` and
  `task_output_result_line` to `testing/transcript_lines.rs`, next to the
  `task_notification_*` helpers, carrying the real-world body shape above.
- A sibling negative test: a `<status>running</status>` retrieval and an
  errored `TaskOutput` result both leave the entry running.
- `hooks/tests/`: mirror `session_end_sweeps_a_lingering_background_subagent.rs`
  for the failed-resume branch — a lingering background entry is swept and
  `SubagentFinished` accompanies the `SpawnFailed`.

Run `make check` and fix whatever it reports.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Folding a successful terminal `TaskOutput` retrieval clears the
      background running entry and broadcasts `SubagentFinished`
      (`background_task_output_retrieval_clears_the_running_subagent`), and
      `delta-attribution` references `TaskOutput` (gate appended).
- [x] A `running`-status poll and an errored `TaskOutput` result leave the
      entry running (negative test above).
- [x] The failed-resume `SessionEnd` branch sweeps lingering background
      entries, asserted by a hooks-level test mirroring
      `session_end_sweeps_a_lingering_background_subagent`.
- [x] e2e-fake `subagent-running-task-output.spec.ts` (gate appended) drives
      the full lifecycle through fake-claude's new `task_output` step: the
      navigator spinner shows during the background run and clears after the
      retrieval turn, with no `task_notification` in the scenario.

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real Claude Code session, launch a background subagent, retrieve
      its result with a blocking `TaskOutput` (no task-notification fires),
      and confirm the navigator spinner clears once the retrieval folds.

## Out of scope

- A watchdog / age-based sweep for background entries whose process dies
  without a `SessionEnd` hook (hard kill) — separate design decision, tracked
  in the plan.
- The `<task-notification>`-with-no-keys leak (already logged with a
  `tracing::warn!`) and any correlation change for it.
- Frontend changes — the existing `SubagentFinished` broadcast and REST
  re-seed paths are sufficient once the server clears the entry.
- Adding `TaskOutput` to `SUBAGENT_TOOL_NAMES` or treating it as a launch.
