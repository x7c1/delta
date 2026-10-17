---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1017-1811-keep-background-subagents-running-across-a-re-adoption
created_at: 2026-10-17T09:11:12Z
updated_at: 2026-10-17T09:35:56Z
---

# fix(server): keep background subagents running across a re-adoption

## Overview

When Delta restarts while a session's pane survives (the desktop app is quit
and reopened, or upgraded), the server re-adopts the pane
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/adopt_pane.rs`,
`adopt_live_pane`). A BACKGROUND subagent the session launched before the
restart keeps running in the pane, but Delta stops showing it: the navigator
row's spinner and the conversation's running-subagent indicator stay dark until
the subagent's completion notification arrives.

Cause: the in-memory running-subagent set
(`session_actor/runtime/subagents.rs`, `RunningSubagent`, filled by
`start_subagent`) is only ever filled when the transcript fold sees the launch
line (`interactor/sync/sync_transcript.rs`, the `start_subagent` call) or from
the `PreToolUse` hook. After a restart the catch-up sync resumes from the
stored line cursor, which is already past the launch line, so the set stays
empty. `GET /api/sessions/{id}/sends` then reports `running_subagents: []`
and the web client's `seedRunningSubagents` clears the indicator. The launch
itself is still persisted: `subagent_launch` rows survive the restart and are
cleared on completion, but they are read back only by
`outstanding_subagent_launches` to reseed attribution's launched-thread map
(`interactor/sync/conversation_source.rs`), never the running set. The doc
comment in `interactor/sweep_running_subagents.rs` ("a resume never rebuilds
the in-memory `running_subagents` set from persisted launch rows") describes
exactly this gap.

Fix: when a pane is re-adopted (boot re-adoption, and the resume backstop in
`open_session`, which both go through `adopt_live_pane`), rebuild the running
set from the session's outstanding launch rows, so the indicator is lit again
for every background subagent still in flight, with the same `thread_id`,
`tool_use_id`, `task_id`, `subagent_type`, `description` and `background` it
had before the restart. The launch row does not persist `subagent_type`,
`description` or `background` today, so it needs to carry them. Decide how,
following the repository's migration conventions in
`backend/crates/gateway/delta-sqlite/src/migrations/`; rows written before the
migration may lack them and must still restore with sensible defaults.

Constraints to respect:

- Only launches that drive the running-subagent indicator may be restored.
  Check what else writes `subagent_launch` rows (the attribution comments
  mention background `Bash` launches being reseeded for task-notification
  routing). A launch that never lit the indicator before the restart must not
  light it afterwards.
- Foreground subagents cannot outlive their turn, and re-adoption closes the
  turn (`TurnInput::Close`) before the catch-up, so a restored entry must
  survive that sweep only if it is a background one.
- A completion that the catch-up sync ingests (the subagent finished while
  Delta was down) must leave the entry finished, not running.
- Connected clients learn the restored set through the existing paths
  (`running_subagents` in the sends envelope, and `SubagentStarted` where it
  applies). Do not add a new wire shape unless one is needed.
- Update the doc comment in `interactor/sweep_running_subagents.rs` and any
  other comment that states the running set is never rebuilt from launch rows.

Out of scope: a running FOREGROUND subagent in a turn that was still in flight
when Delta restarted (re-adoption closes that turn by design).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A usecase test re-adopts a session that has an outstanding BACKGROUND
      subagent launch row (launch line already behind the cursor) and asserts
      that the session's running-subagent set (as reported for the sends
      envelope) contains it, with the launching thread, `tool_use_id`,
      `subagent_type`, `description` and `background: true`.
- [x] A usecase test re-adopts a session whose outstanding background launch
      completes in the catch-up (its `<task-notification>` is among the
      newly ingested lines) and asserts that the running set is empty
      afterwards.
- [x] A test covers a persisted launch row that does not drive the indicator
      (e.g. a background `Bash` launch, if such rows exist) and asserts that
      re-adoption does not add it to the running set.
- [x] A `delta-sqlite` test covers the launch row's new persisted fields:
      round-trip of the new values, and a pre-migration row read back with
      defaults.

### Before merge (verified outside the check command)

- [ ] In `make dev`, a session that launches a background subagent which runs
      long enough to survive a server restart (only the `delta-server`
      process is restarted; the dev tmux server and its pane keep running —
      `make down` would kill them) shows the navigator spinner and the running
      indicator again after the restart, and clears them when the subagent
      completes.
