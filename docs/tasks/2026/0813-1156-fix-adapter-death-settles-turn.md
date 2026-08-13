---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0813-1156-fix-adapter-death-settles-turn
created_at: 2026-08-13T11:56:00Z
updated_at: 2026-08-13T15:50:00Z
---

# fix(agent): settle the turn and pending permissions when an adapter session's connection dies

## Overview

When an adapter-backed session's `codex app-server` process dies mid-turn,
Delta currently notices only at the transport layer: the connection reader
sees EOF, stops, and clears the pending request map
(`backend/crates/gateway/codex-agent/src/lib.rs`, reader loop — in-flight
`request()` calls resolve to `ConnectionClosed`). Nothing propagates to the
session's runtime: the turn stays `in_flight` forever, the pending-permission
queue keeps entries whose wire requests no longer exist, and the session still
reports `open: true`.

Observed twice in dogfooding (2026-08-13, real `codex app-server` processes
killed mid-turn):

- Session A: turn stuck `in_flight` indefinitely; the sends envelope kept
  reporting a live-looking session; queued sends would defer forever behind a
  turn that can never complete.
- Session B: an approval dialog stayed on screen after the process died. The
  user's Allow marked the row decided, then the wire write failed
  (`failed to write to app-server: Broken pipe (os error 32)`, HTTP 500) and
  the dialog stayed (no `permission_resolved` ever arrives); the retry got
  `409 permission_not_pending`. From the user's seat this is
  indistinguishable from a hang.

Fix: make adapter-session death a first-class settle event, end to end.
Implementation direction (details are yours, invariants are not):

- **Detect at the right layer.** The adapter owns the connection: when the
  per-session event stream ends because the underlying process/connection
  died (reader EOF / read error — as opposed to an orderly `close()`), the
  adapter must surface a terminal event on the session's `AgentEvent` stream
  (e.g. a session-ended/failed variant) instead of the stream just going
  silent. Distinguish orderly close from death so a normal close does not
  produce a spurious failure signal.
- **Settle the runtime.** The usecase pump, on that terminal event, settles
  everything a dead connection strands, reusing the existing reducers where
  they exist:
  - the turn machine → the same path an interrupted/failed turn takes today
    (`SessionEvent::TurnInterrupted` clears the stuck chip; see
    `complete_agent_turn` in
    `backend/crates/domain/delta-usecase/src/interactor/agent_event.rs`);
  - the pending-permission queue → cleared with the client-visible
    resolution signals (the queue and its clear-on-idle behavior live in
    `backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/permission.rs`);
  - the row/token bookkeeping → still-pending `permission_request` rows for
    that session must not be left `pending` forever (decide on and document a
    disposition — e.g. denied with a reason — so the audit trail says what
    happened), and the in-memory token correlations are dropped;
  - the session itself → marked closed (the UI already renders closed
    sessions as view-only, and the next Send resumes them — that existing
    resume path is the recovery story and must keep working).
- **Broadcast so a live browser converges without a refetch.** A browser
  watching the session must see the turn end and the dialog clear from the
  event stream alone; a reconnecting browser must see the same truth from
  the sends envelope + session list refetch.
- **fake-codex coverage.** Drive the real stack: a fake-codex turn that
  raises an approval and then the fake process is killed (or a scenario step
  makes it exit) — assert the settle: turn interrupted, dialog cleared, no
  `pending` row left, session reports closed, and a subsequent Send resumes
  the session and completes a fresh turn.

Operation × state coverage (connection death vs session state — write a test
per row):

- Death mid-turn with no pending permission → turn settles as interrupted;
  session closes.
- Death mid-turn with N pending permissions → all N clear (client-visible),
  no `permission_request` row left `pending`, decision POSTs for them answer
  409 (not 500), turn settles, session closes.
- Death while idle (no turn in flight) → session closes; no spurious
  turn-interrupted signal.
- Orderly `close()` (existing path) → behavior unchanged; no double
  session-closed signal, no spurious failure event.
- Send to the closed session afterwards → the existing resume path spawns a
  fresh process and the new turn runs to completion (pin with fake-codex).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Adapter-level test: killing the fake-codex process mid-turn surfaces a
      terminal event on the session's event stream (not silence); an orderly
      `close()` does not produce the failure variant.
- [x] Server-level test against fake-codex: death mid-turn with pending
      approvals settles everything — `turn_interrupted` broadcast, permission
      dialog cleared client-visibly, no `permission_request` row left
      `pending`, session reports closed in the session list, and a decision
      POST for a stranded request answers 409.
- [x] Server-level test: a Send to the settled (closed) session resumes it
      and a fresh turn completes over the real fake-codex stack.
- [x] Idle-death and orderly-close rows are unit/server tested: no spurious
      turn-interrupted on idle death, no behavior change on orderly close.
- [x] The sends envelope of a settled session reports `turn: idle` (or the
      settled equivalent), `permission: null`, `permission_count: 0` after
      the death — asserted in the server-level test via refetch.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real `codex app-server` session: kill the app-server process
      (by PID) while a turn is in flight with a pending approval dialog — the
      dialog clears and the turn settles in the UI without a reload, the
      session shows as closed, and sending a new message resumes it
      successfully.

## Out of scope

- The capability-aware 409 fallback wording in the permission notice
  (separate task; with this fix the dead-dialog route to that 409 mostly
  disappears, but the guidance fix stands on its own).
- Auto-respawn/retry of a died adapter process (recovery stays explicit:
  the next Send resumes).
- Pane-backed (tmux/Claude) session death handling — already covered by the
  `SessionEnd` hook and the launch watchdog.
- The generic invariant-watchdog design: this task settles the one concrete
  class it reproduces; the watchdog generalization stays a separate design
  task.
