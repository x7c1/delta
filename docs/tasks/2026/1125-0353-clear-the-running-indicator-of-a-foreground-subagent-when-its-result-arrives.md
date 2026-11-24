---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1125-0353-clear-the-running-indicator-of-a-foreground-subagent-when-its-result-arrives
created_at: 2026-11-24T18:53:07Z
updated_at: 2026-11-24T19:10:03Z
---

# fix(attribution): clear a foreground subagent's running indicator when its result arrives

## Overview

`launches_in_background` in
`backend/crates/domain/delta-attribution/src/claude_format/subagent_launch.rs`
treats an `Agent`/`Task` tool_use with no `run_in_background` key as a
background launch, because modern Claude Code makes those calls async by
default and dropped the key from the schema. Some calls still run in the
foreground without the key, though: Claude Code decides per subagent type
(a `web-fetch` agent, for example, always runs in the foreground), and an
explicit `run_in_background: false` is honoured already but an absent key is
not. For such a call:

- the content-block fold (`attribute/content_blocks.rs`) records it in
  `launched_threads` and emits `Effect::SubagentLaunched` and a
  `SubagentIndicatorStarted { background: true, .. }`;
- the `PostToolUse(Agent)` hook (`delta-usecase/src/interactor/hooks/on_post_tool_use.rs`)
  finds no foreground entry, so it only upgrades the `agentId`;
- no `<task-notification>` ever arrives, because the subagent already
  finished when its tool_result came back, and the turn-end sweep keeps
  background entries.

So the running indicator for that subagent stays on forever, and reloading
re-seeds it from the persisted launch row. Observed in a real session on
2026-11-23 (a foreground `web-fetch` Agent call).

The tool_result of a background launch and of a foreground run differ in
what Claude Code writes. Recorded from a real Claude Code transcript
(2026-11-24):

- background launch: the `tool_result` text starts with
  `Async agent launched successfully.` followed by `agentId: <id> ...`
  (see the fixture in `delta-attribution/tests/support/mod.rs`); the JSONL
  sibling `toolUseResult.status` is `async_launched`.
- foreground run: the `tool_result` content is the subagent's final report
  (arbitrary text, which may also carry an `agentId: <id>` line); the JSONL
  sibling `toolUseResult` carries `"status": "completed"` together with
  `agentId`, `totalDurationMs`, `totalTokens` and `usage`. The
  `PostToolUse` hook's `tool_response` is that same object.

Fix it so that, once the launching call's result shows the subagent ran in
the foreground, the running entry is finished (a `SubagentFinished` /
`SubagentCompleted` the client already handles) and the persisted launch row
is removed, on both paths:

- the live path: `on_post_tool_use` with a `tool_response` whose `status` is
  `completed` for an entry that was recorded as background;
- the fold path (cold-start replay / re-fold without the hook): a non-error
  `tool_result` for a recorded launch whose content is not an async-launch
  acknowledgement. Because the parser keeps only the `content` blocks, the
  fold has to decide from the text; recognise the async-launch
  acknowledgement by its `Async agent launched` marker, and keep the
  current behaviour (record the `agentId`, keep the entry running) for it.

Keep the predicate in one place next to `agent_id_from_tool_result_content`
in `claude_format`, so the hook and the fold do not each grow their own
string match. A background `Bash` (`run_in_background: true`) also returns
an immediate acknowledgement and is completed by a `<task-notification>`;
it must not be finished by this change — scope the new rule to the subagent
tools (`is_subagent_tool`). An errored launch keeps its existing handling.
Update `fake-claude` (`backend/crates/apps/fake-claude/src/run/tool_steps.rs`
mirrors `launches_in_background`) if it needs to emit a foreground result
for the tests.

States of the running entry when the result arrives, each covered by a test:

- entry recorded as background, result is the async-launch acknowledgement →
  stays running, `agentId` recorded (unchanged behaviour);
- entry recorded as background, result is a foreground report → finished and
  its launch row removed, by the hook and by the fold;
- entry recorded as foreground (explicit `run_in_background: false`) →
  finished by `PostToolUse` as today;
- background `Bash` launch, result is its immediate acknowledgement → stays
  running.

Keep every backend source file within the 1000-line limit (`make check-backend-file-length`;
split a test module by responsibility rather than growing it), and do not link a private item
from a public item's doc comment (`make check-backend-doc` runs rustdoc with `-D warnings`).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A unit test in `delta-attribution` shows that folding an `Agent`
      tool_use with no `run_in_background` key followed by a non-error
      tool_result whose text is a subagent report (no `Async agent launched`
      marker) emits the effect that finishes the running entry and drops it
      from `launched_threads`.
- [x] A unit test shows that the same fold with an `Async agent launched
      successfully.\nagentId: <id>` tool_result keeps the entry running and
      records the `agentId`.
- [x] A test of `on_post_tool_use` shows that a `PostToolUse(Agent)` whose
      `tool_response` has `"status": "completed"` for an entry recorded as
      background finishes it, broadcasts `SubagentFinished`, and removes the
      persisted launch row; a `tool_response` with `"status":
      "async_launched"` leaves it running.
- [x] A test shows that a background `Bash` launch is not finished by its
      immediate tool_result.
