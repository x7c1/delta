---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq 'fn a_codex_turn_announces_its_start' backend/crates/domain/delta-usecase/src/ && ! grep -q 'cannot drift apart' backend/crates/domain/delta-usecase/src/ports/session_store/mod.rs"
assignee: null
branch: task/1025-1524-announce-codex-turn-starts-to-the-browser
created_at: 2026-10-25T15:24:57Z
updated_at: 2026-10-25T15:54:31Z
---

# fix(server): announce Codex turn starts so the session list shows them running

## Overview

The navigator's running spinner is lit by the `turn_started` WebSocket event
(`frontend/packages/apps/web/src/store/live/runningThreadsSlice.ts`) and, for
the open session only, by the `/sends` seed. Every pane-backed (Claude) turn
now announces its start from `on_user_prompt_submit`, but an adapter-backed
(Codex) session never emits `SessionEvent::TurnStarted` at all:

- `interactor/lifecycle/adapter_session/agent_turn.rs`, `start_agent_turn`,
  feeds `TurnInput::PromptSubmitted { send_id: None }` to the turn machine
  after `adapter.send` acknowledges the turn, and marks the send row matched,
  but broadcasts nothing.
- `interactor/agent_event.rs`, `on_agent_event`, drops
  `AgentEvent::TurnStarted` in the `_ => {}` arm ("turn start — already
  applied at send").

So a Codex session that is not open shows no spinner while its turn runs,
though `complete_agent_turn` does emit `TurnCompleted` / `TurnInterrupted` at
the end. Opening the card lights it through the seed, which is the same
symptom the Claude fix removed.

### The change

1. **Announce the Codex turn start.** Emit `SessionEvent::TurnStarted` once
   per adapter-backed turn, at the point where the turn machine goes
   `InFlight` (`start_agent_turn`, shared by the opening turn in
   `activate_adapter_session.rs`, `dispatch_agent_turn` and
   `dispatch_queued.rs`). Name the send (`send_id: Some(send_id)`) and the
   send's own `thread_id`, so the start and the end
   (`in_progress_turn_thread` in `complete_agent_turn`) name the same thread.
   For `matched_uuid`, decide between `None` and the id the row is marked
   matched with; the browser does not read it, but the api-mocks store
   copies it onto the send row, so pick the one that keeps the mock agreeing
   with what `/sends` reports for the row, and say why in the doc comment.
   Emit it with the same mechanism `complete_agent_turn` uses
   (`emit_async_event`) unless the call path makes another one clearly
   right. Do not also emit from `AgentEvent::TurnStarted`; one announcement
   per turn. Update the comment in `on_agent_event`'s `_` arm and the
   module docs of `agent_turn.rs` accordingly.
2. **Fix the doc comments that say this event is pane-only.**
   `SessionEvent::TurnStarted` in `ports/session_event.rs` says "Pane-backed
   sessions only, too: an adapter-backed session has no prompt hook to fire";
   rewrite it for the new producer. Check the wire type's doc
   (`backend/crates/gateway/delta-wire/src/session_event/mod.rs`) and
   `docs/guides/api/live-channels.md` for the same claim.
3. **Pin the api-mocks behavior for a mid-turn prompt that names no send.**
   In `frontend/packages/testing/api-mocks/src/handlers.ts`, the
   `turn_started` branch sets the active turn's `sendId` to `event.send_id`,
   so a mid-turn `turn_started` with `send_id: null` (a background-task
   notification arriving while the turn runs) clears it. The real server
   does the same: `(InFlight, PromptSubmitted { send_id })` in
   `turn/transition.rs` moves to `InFlight { send_id }`, and `/sends`
   reports the machine's `send_id`. Leave the handler as it is, and add an
   api-mocks test that pins this.
4. **Fix the stale doc of `in_progress_turn_thread`.** In
   `backend/crates/domain/delta-usecase/src/ports/session_store/mod.rs`, the
   doc says it is a default method "so the composition lives in one place and
   the two hook call sites cannot drift apart". Its callers now go well
   beyond two hooks (the prompt and stop hooks, the transcript sync, the
   live-state query, the Codex turn end). Rewrite that sentence to state the
   real reason without enumerating callers that will drift again.

### Out of scope

- The WebSocket reconnect path (`resetTurnEphemera` clears every session's
  running flags and only the open session re-seeds).
- Any change to how Codex turns end.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `a_codex_turn_announces_its_start` (delta-usecase, next to
      `interactor/enqueue/tests/codex_second_send_dispatches_over_the_adapter.rs`
      and using the same fake agent): a send to an adapter-backed session
      produces exactly one `SessionEvent::TurnStarted` carrying that send's id
      and thread, before the turn's `TurnCompleted`.
- [x] The opening turn of a Codex session also emits `TurnStarted` (covered
      by the same test or an extended existing opening-turn test).
- [x] An api-mocks test pins that a mid-turn `turn_started` naming no send
      leaves the sends envelope's `turn.send_id` `null`, as the real server
      reports it.
- [x] The `in_progress_turn_thread` doc no longer says "cannot drift apart"
      (grep gate in the check command).
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] In `make dev`, a Codex session that is not focused shows the running
      spinner in the session list while its turn runs, without clicking it,
      and the spinner goes out when the turn ends.
