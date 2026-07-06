---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0706-0726-fix-restart-zombie-dispatched-send
created_at: 2026-07-06T07:26:51Z
updated_at: 2026-07-06T15:24:47Z
---

# fix(usecase): reconcile dispatched sends orphaned by a server restart

## Overview

A `send` row can survive a server restart in status `dispatched` while the
turn state that owned it does not: turn state is runtime-only and is rebuilt
`Idle` on boot (see the module docs in
`backend/crates/domain/delta-usecase/src/turn.rs`, which claim this is
"correct by construction" — the claim misses that the *send rows* half of the
single-outstanding invariant is persistent). The orphaned row becomes a
zombie: it is the oldest `dispatched` row, so
`head_dispatched_send` (used for `UserPromptSubmit` correlation in
`backend/crates/domain/delta-usecase/src/interactor/hooks/on_user_prompt_submit.rs`)
returns it forever. Every subsequent send then dispatches, mismatches
against the zombie's text, is treated as an external prompt, and gets
requeued by the turn machine's anomaly path — an **infinite requeue loop
that re-submits the same prompt to the TUI on every turn end** (observed:
the same message submitted 5 times in a row) until the user cancels the
new send. The zombie itself is unrecoverable from the UI: `cancel_send`
(`backend/crates/domain/delta-usecase/src/interactor/cancel_send.rs`)
only honours a dispatched cancel while the turn is `AwaitingEcho` with a
matching `send_id`, which is impossible after a restart, so it returns
`SendNotCancellable` — and the frontend swallows that error (see below),
making the cancel button appear dead.

Observed in dogfooding on 2026-07-06: a send was dispatched into a TUI
that had stopped at its usage limit, so its keystrokes were swallowed and
no `UserPromptSubmit` echo ever arrived; the server was then restarted;
the first send after the resume entered the requeue loop described above.
The comment in
`backend/crates/domain/delta-usecase/src/interactor/enqueue/enqueue_into_open.rs`
already names a correlation-shadowing dispatched row as the hazard its
dispatch-failure guard exists to clear — this task closes the same hazard
on the path that crosses a process lifetime.

### What changes

Three fixes, one backend invariant restore plus two UX hardenings:

1. **Boot-time reconcile (the core fix).** At server boot, before any
   session actor exists, every `dispatched` row is an orphan by
   definition — no turn machine can be awaiting its echo. Add a
   `SessionStore` method (e.g. `requeue_all_dispatched()`, implemented in
   `backend/crates/gateway/delta-sqlite/src/store.rs` as a single
   `UPDATE send SET status = 'queued' WHERE status = 'dispatched'`) and
   call it once during bootstrap
   (`backend/crates/libs/delta-bootstrap/src/lib.rs`). Requeue rather
   than cancel: it matches the existing `OrphanedSend::Requeue`
   philosophy ("a composed message is never silently lost") — the send
   re-dispatches intact through the normal idle-dispatch path
   (`dispatch_queued_send`) the next time its session is open and idle.
   Update the `turn.rs` module docs ("Runtime-only, never persisted"
   section) to state the actual invariant: rebuilt-Idle-on-boot is sound
   *because* boot also requeues every persisted `dispatched` row.

2. **Cancel escape hatch for ownerless dispatched rows.** In
   `cancel_send.rs`, when the target row is `dispatched` but the turn
   state holds no claim on it (anything other than
   `AwaitingEcho { send_id }` for that row), cancel it as a pure state
   transition: flip the row to `cancelled` without injecting the Escape
   keystroke (there is no composer buffer to discard that Delta knows
   about) and without touching the turn machine. The existing
   `AwaitingEcho`-matching path keeps its Escape + `TurnInput::Cancel`
   behaviour; the existing `InFlight` rejection stays (an echoed send is
   owned by its transcript line). With fix 1 in place this state should
   be nearly unreachable, but it is the correct escape hatch if the
   invariant is ever violated again.

3. **Surface cancel failures in the UI.** `useCancelSendMutation` in
   `frontend/packages/gateway/api-client/src/query-hooks.ts` only
   invalidates the open-send list in `onSettled`; a `send_not_cancellable`
   error produces no user-visible feedback, so a rejected cancel looks
   like a dead button. Add an `onError` (or handle the error at the
   call site in
   `frontend/packages/apps/web/src/features/composer/PendingQueue.tsx`)
   that surfaces the failure through the existing `ErrorSnackbar` +
   `useNotificationStore` pattern introduced by the open-cwd feature, so
   a silent no-op becomes an explained refusal.

### Why this design

- **Boot sweep over actor-open reconcile.** At boot, *all* dispatched
  rows are orphans (no actors exist yet), so one store-level sweep is
  exact, covers sessions that are never reopened, and needs no per-actor
  bookkeeping. Reconciling at actor open instead would leave zombies of
  never-reopened sessions pinned in the navigator.
- **Requeue over cancel.** Cancelling would drop the user's composed
  message with no trace; requeueing re-types it when the session next
  goes idle — the same disposition the turn machine already chooses for
  a never-echoed send (`OrphanedSend::Requeue`).
- **Keep the compact-path redispatch as is.** The existing
  `redispatch_stuck_dispatched` (compact recovery) re-types while staying
  `dispatched`, because there the turn machine still owns the row. The
  restart case is different — ownership is gone — so `queued` is the
  honest state.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `SessionStore::requeue_all_dispatched()` (or equivalently named)
      method exists, implemented for the sqlite store and the test fake,
      and bootstrap calls it exactly once at startup. A store-level test
      seeds `dispatched` rows across two sessions and asserts both are
      `queued` afterwards while `matched` / `cancelled` rows are
      untouched.
- [x] An interactor-level test reproduces the head-of-line scenario:
      seed a stale `dispatched` row (as left by a dead process), run the
      boot reconcile, open the session, enqueue a new send, and drive its
      `UserPromptSubmit` echo — the new send resolves `matched` (no
      requeue loop), and the stale row re-dispatches afterwards through
      the normal idle path in FIFO order.
- [x] Cancelling a `dispatched` send whose id the turn machine does not
      own flips the row to `cancelled`, injects **no** keystrokes into
      the pane (asserted against the fake tmux driver), and leaves the
      turn state unchanged. The existing owned-cancel test
      (Escape + `TurnInput::Cancel`) stays green.
- [x] A frontend test asserts that a failed cancel-send mutation surfaces
      a notification via the existing notification store (the
      `send_not_cancellable` path no longer fails silently).
- [x] The `turn.rs` module docs no longer claim the Idle-on-boot rebuild
      is sound in isolation; they document the boot-time requeue as the
      other half of the invariant.
- [x] `make check` passes (backend fmt/build/test/clippy + generated
      bindings freshness + frontend build/typecheck/test/lint; also the
      configured `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real `make dev` loop: dispatch a send that never echoes (e.g.
      into a TUI stopped at a usage limit or otherwise not accepting
      input), restart the server, reopen the session — the previously
      stuck chip re-dispatches once the session is idle, and subsequent
      sends match normally instead of entering a requeue loop.
- [ ] Pressing cancel on a pending chip that the backend refuses to
      cancel produces visible feedback (snackbar) instead of doing
      nothing.

## Out of scope

- The upstream trigger — Claude Code's TUI swallowing keystrokes while
  stopped at a usage limit — is not fixable from Delta; this task makes
  the aftermath recoverable instead.
- Extending the compact-path `redispatch_stuck_dispatched` to
  `SessionStart(source=resume)` or other mid-life sources. With the boot
  sweep in place the restart case no longer needs it, and the compact
  path's semantics (re-type while staying `dispatched`) are correct for
  its own scenario.
- A general error-toast audit of other mutations that fail silently.
  This task wires the cancel-send path only; a broader sweep deserves
  its own task.
