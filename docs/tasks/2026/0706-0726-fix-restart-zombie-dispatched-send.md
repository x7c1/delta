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
updated_at: 2026-07-08T14:26:03Z
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

1. **Boot-time restore + explicit release (the core fix).** At server
   boot, before any session actor exists, every `dispatched` row is an
   orphan by definition — no turn machine can be awaiting its echo. A
   `SessionStore::restore_all_dispatched()` sweep (delta-sqlite: a single
   guarded UPDATE) flips each one to `status='queued'` with a
   `restored_at` marker (new nullable column, added via the additive
   migration mechanism), and bootstrap calls it exactly once at startup.
   Restored rows are **never auto-sent**: `next_queued_send` — the single
   selection every auto-dispatch trigger goes through — skips rows whose
   `restored_at` is set, so a message composed before a restart (possibly
   days old) cannot silently re-submit itself into a conversation that
   has moved on. Instead the UI surfaces it as "Restored after restart"
   with explicit Send / Cancel actions; Send calls the new
   `POST /api/sends/{id}/release` endpoint (guarded UPDATE clearing the
   marker, 409 `send_not_releasable` when the row is not a restored
   queued row), which drops the row into the normal queued flow — an
   open, idle session types it immediately, otherwise it dispatches on
   the next idle trigger.

   Two supporting fixes make that queued flow trustworthy across
   restarts and resumes: `dispatch_queued_send` defers while the session
   is inside the resume-readiness window (previously an idle-flush could
   type a queued row into a freshly-bound pane that was not yet accepting
   input, silently losing the keystrokes), and `dispatch_ready_resume`
   flushes the queued backlog once a resume settles (previously nothing
   promoted a queued row on reopen until an unrelated trigger fired).
   The `turn.rs` module docs ("Runtime-only, never persisted" section)
   now state the actual invariant: rebuilt-Idle-on-boot is sound
   *because* boot also restores every persisted `dispatched` row.

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
  bookkeeping.
- **Restore-and-ask over auto-resend (and over cancel).** Cancelling
  would drop the user's composed message with no trace; auto-resending
  (the first iteration of this change) surprised on-hardware review — a
  stale message resurrected itself the moment the session reopened, and
  could even land *after* a newer message the user had just sent. The
  restored marker keeps the message visible and intact while leaving the
  decision to send, edit-and-resend (future work), or discard with the
  user.
- **A marker column over a new status value.** The `send` table is
  STRICT with a CHECK on `status`; a fifth status would force a table
  rebuild. `status='queued'` + `restored_at` keeps every existing
  queued-row invariant (cancel guard, FIFO id order) and needs only an
  additive column.
- **Keep the compact-path redispatch as is.** The existing
  `redispatch_stuck_dispatched` (compact recovery) re-types while staying
  `dispatched`, because there the turn machine still owns the row. The
  restart case is different — ownership is gone — so a visible restored
  row is the honest state.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `SessionStore::restore_all_dispatched()` method exists,
      implemented for the sqlite store and the test fake, and bootstrap
      calls it exactly once at startup (pinned by a file-backed bootstrap
      test). Store-level tests seed `dispatched` rows across two sessions
      and assert both become `queued` with `restored_at` set while
      `matched` / `cancelled` / plain `queued` rows are untouched, and
      that the additive `restored_at` migration applies to a pre-column
      database.
- [x] Restored rows never auto-dispatch: `next_queued_send` skips rows
      with `restored_at` set, and an interactor test drives boot restore
      → session open → resume settle and asserts zero keystrokes reach
      the pane.
- [x] `POST /api/sends/{id}/release` clears the marker only for a
      restored queued row and returns 409 with stable code
      `send_not_releasable` otherwise (unknown / not restored / already
      released); after a successful release on an open idle session the
      row dispatches through the normal path and its echo resolves it
      `matched` (interactor test).
- [x] Genuinely queued (non-restored) rows are not stranded by a
      restart or resume: `dispatch_queued_send` defers during the
      resume-readiness window (no keystrokes typed into a cold pane) and
      the queued backlog flushes once the resume settles (interactor
      tests).
- [x] Cancelling a `dispatched` send whose id the turn machine does not
      own flips the row to `cancelled`, injects **no** keystrokes into
      the pane (asserted against the fake tmux driver), and leaves the
      turn state unchanged; cancel also covers restored rows. The
      existing owned-cancel test (Escape + `TurnInput::Cancel`) stays
      green.
- [x] Frontend tests assert that a restored send renders the
      "Restored after restart" affordance with an explicit Send action,
      and that failed release-send and cancel-send mutations surface a
      notification via the existing notification store (no silent
      failures).
- [x] The `turn.rs` module docs no longer claim the Idle-on-boot rebuild
      is sound in isolation; they document the boot-time restore as the
      other half of the invariant.
- [x] `make check` passes (backend fmt/build/test/clippy + generated
      bindings freshness incl. the regenerated `Send` wire type +
      frontend build/typecheck/test/lint; also the configured
      `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real `make dev` loop: leave a `dispatched` row behind (e.g.
      a send typed into a TUI that never accepted it, or a row flipped
      by hand while the server is down), restart the server, reopen the
      session — the row appears as "Restored after restart" with Send /
      Cancel, is NOT sent automatically, and pressing Send delivers it
      through the normal flow while subsequent sends match normally.
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
