---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq 'fn a_task_notification_turn_after_stop_announces_its_start' backend/crates/domain/delta-usecase/src/ && grep -rq 'fn a_send_whose_line_is_not_written_yet_announces_its_start' backend/crates/domain/delta-usecase/src/ && grep -rq 'fn a_prompt_typed_into_the_pane_announces_its_start' backend/crates/domain/delta-usecase/src/"
assignee: null
branch: task/1025-1237-announce-every-turn-start-to-the-browser
created_at: 2026-10-25T12:37:34Z
updated_at: 2026-10-25T13:55:00Z
---

# fix(server): announce every turn start so the session list shows it running

## Overview

The navigator's running spinner on a session card is lit by exactly one live
signal, the `turn_started` WebSocket event
(`frontend/packages/apps/web/src/store/live/runningThreadsSlice.ts`,
`reduceTurnStarted`), and cleared by `turn_completed` / `turn_interrupted`.
The only other way it lights is `seedActiveTurn`, which reads `turn` from
`GET /api/sessions/{id}/sends` and runs only for the session whose
conversation pane is open (`features/composer/useSendActivity.ts`).

The server, however, broadcasts `SessionEvent::TurnStarted` only from
`backend/crates/domain/delta-usecase/src/interactor/hooks/on_user_prompt_submit.rs`,
and only when the prompt consumed the outstanding send, its text echoes the
send (`prompt_echoes_send`), AND the send's user line was already in the
JSONL during this hook's sync (`match_uuid_for_prompt`). In every other
`UserPromptSubmit`, the turn machine still moves to `InFlight`
(`turn/transition.rs`, `(Idle, PromptSubmitted)`) and `QueryLiveState`
resolves the in-progress thread, yet nothing is broadcast. So a session that
is not open loses its spinner at the previous turn's `Stop` and does not get
it back while the new turn runs; clicking the card mounts the pane, whose
`/sends` fetch reports `in_flight` and lights it. Users read the vanished
spinner as "finished".

The three shapes that hit this, all reproduced by tests:

1. **A background-task notification after `Stop`** (the dominant case in
   practice). A background subagent or `run_in_background` Bash finishes
   after the turn ended; the harness submits a `<task-notification>` prompt,
   which consumes no send, so the `None` arm runs and only suppresses
   `ExternalInput`.
2. **A Delta send whose user line is not in the JSONL yet** when its
   `UserPromptSubmit` arrives — the code comments call this "the common
   timing case". This includes a queued send that `on_stop` dispatched the
   moment the previous turn ended.
3. **A prompt typed straight into the pane** (only `ExternalInput` goes
   out).

### The change

Broadcast `TurnStarted` whenever `on_user_prompt_submit` leaves the turn
machine `InFlight`, with `thread_id` resolved by
`SessionStore::in_progress_turn_thread` (the same resolution `on_stop`,
`sync_transcript` and `QueryLiveState` use, so the start and the end of a
turn name the same thread). Resolve it at the point where the result is
right for each shape — after the sync, so a notification line or a typed
line already ingested is attributed — and keep the send-bound case naming
the send's own `thread_id` as today.

`send_id` and `matched_uuid` become optional on both
`ports::SessionEvent::TurnStarted` and the wire type
(`backend/crates/gateway/delta-wire/src/session_event/mod.rs` and
`from_session_event.rs`): present exactly when they are known today, `None`
otherwise. Regenerate the TypeScript bindings
(`frontend/packages/gateway/wire-gen/src/generated/`) and update every
consumer and fixture of `turn_started` (`grep -rn turn_started frontend/packages --include='*.ts' --include='*.tsx'`,
excluding `dist`), including `testing/api-mocks`. The browser reducer only
needs `session_id` and `thread_id`, so its behavior is unchanged.

A mid-turn `<task-notification>` (the machine is already `InFlight`) may
also announce: `reduceTurnStarted` is idempotent, and announcing every time
keeps the rule simple — "the hook left the machine `InFlight`" — rather than
tracking the previous state. Choose whichever is clearer and say which in
the doc comment.

Update the doc comments that describe the old rule: `SessionEvent::TurnStarted`
and `SessionEvent::ExternalInput` in `ports/session_event.rs`, the
`on_user_prompt_submit` doc and inline comments, the wire type's doc, and the
frontend comments that say `turn_started` is the turn's announcement only for
matched sends (e.g. `runningThreadsSlice.ts` "Note `turn_started` only fires
when ...").

Existing tests assert the old rule and must change their expectation, not
be deleted — e.g.
`interactor/enqueue/tests/mismatched_prompt_consumes_the_outstanding_send.rs`
(asserts no `TurnStarted`), and check
`interactor/sync/tests/branch_send_attributes_late_arriving_lines_to_child.rs`,
`interactor/enqueue/tests/outstanding_send_matches_and_marks_send.rs` and
`interactor/enqueue/tests/composer_first_send_writes_real_rows_before_spawn.rs`.
`grep -rn TurnStarted backend/crates/domain/delta-usecase/src` finds them all.

### Out of scope

- The WebSocket reconnect path: `resetTurnEphemera` clears every session's
  running flags and only the open session re-seeds. Deferred on purpose; it
  is revisited only if the spinner still vanishes after this fix.
- Adapter-backed (Codex) sessions, whose turn start is applied at send time
  and has no prompt hook. If the work shows they have the same gap, say so
  in the report rather than fixing it here.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `a_task_notification_turn_after_stop_announces_its_start`
      (delta-usecase hook tests): a send's turn starts and ends with `Stop`
      (machine `Idle`), then a `<task-notification>` line is in the
      transcript and its `UserPromptSubmit` arrives; the returned events
      contain `TurnStarted` on the main thread with `send_id: None`, and the
      live state is `InFlight` with `in_progress_thread` equal to that same
      thread.
- [x] `a_send_whose_line_is_not_written_yet_announces_its_start`: a dispatched
      send's `UserPromptSubmit` arrives before its user line is in the
      transcript; the events contain `TurnStarted` naming the send's id and
      its thread, with `matched_uuid: None`.
- [x] `a_prompt_typed_into_the_pane_announces_its_start`: a prompt with no
      outstanding send produces both `ExternalInput` and `TurnStarted` on the
      main thread.
- [x] The existing verbatim-echo case still carries `send_id` and
      `matched_uuid` (the `Some` values), and every existing test that
      asserted "no `TurnStarted`" now asserts the new rule.
- [x] A frontend test shows a `turn_started` with `send_id: null` and
      `matched_uuid: null` lights the session row's running spinner
      (`data-testid="session-running"`) on a session that is not focused.
- [x] Generated bindings are fresh and every `turn_started` fixture type-checks
      (both covered by `make check`).

### Before merge (verified outside the check command)

- [x] In `make dev`, a session that is not focused and whose turn is started
      by a background-task notification after `Stop` shows the running
      spinner in the session list while that turn runs, without clicking it.
