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
branch: task/0816-1535-fix-dispatched-send-echo-deadline
created_at: 2026-08-16T15:35:37Z
updated_at: 2026-08-16T17:20:00Z
---

# fix(turn): echo-deadline watchdog parks a dispatched send whose keystrokes vanish without a trace

## Overview

A pane-backed (Claude Code) send got permanently stuck in `dispatched` with
zero signals for the turn machine, blocking the queue behind it. Live
incident post-mortem (dev server, 2026-08-16, Claude Code v2.1.233):

1. The session was idle between turns. Claude Code proactively displayed its
   own interactive onboarding modal (the auto-mode setup flow) in the TUI.
2. Delta dispatched a send: the pasted text was swallowed whole by the modal
   (it appears nowhere in the pane scrollback, the composer, or the
   transcript), and the trailing Enter *accepted the modal instead* —
   `/auto-mode-setup` ran as a `local_command`.
3. No user message was written, no `UserPromptSubmit` hook fired, and no turn
   boundary appeared. The turn machine sat in
   `TurnState::AwaitingEcho { send_id }`
   (`backend/crates/domain/delta-usecase/src/turn.rs`) forever: the send row
   stayed `dispatched`, the browser showed a permanent "In progress", and the
   next send stayed `queued` behind it.

Every existing recovery is event-driven and therefore blind to this class:

- The requeue budget + park path
  (`backend/crates/domain/delta-usecase/src/interactor/turn_input.rs`,
  `claim_requeue` / `park_unechoable_send`) only runs when the transition
  table emits `OrphanedSend::Requeue`, which requires *some* input
  (`ExternalPrompt`, `Stop`, …). Here zero inputs ever arrive.
- The compact re-dispatch
  (`backend/crates/domain/delta-usecase/src/interactor/enqueue/redispatch_stuck.rs`)
  is triggered only by compact-end signals.
- The transition table has no time-based input at all — `AwaitingEcho` can
  only be left by an event.

Fix: add a **deadline watchdog** so "no signal at all" itself becomes a
signal. This closes the whole class (TUI modals, a human pressing Escape in
the attached pane, any future keystroke-swallowing state), not just the
observed modal.

### Design

1. **New input `TurnInput::EchoDeadline { send_id }`** in the transition
   table:
   - `(AwaitingEcho { send_id }, EchoDeadline { same id })` →
     `Idle` + `OrphanedSend::Requeue(send_id)`, **not** anomalous (the
     deadline firing is an expected, designed-for outcome).
   - `EchoDeadline` with a mismatched id, or in any other state, is a stale
     no-op (also not anomalous — a deadline racing a real settle is normal).
     Keep the table exhaustive as it is today.
2. **Deadline sweep riding the existing server watchdog loop.** The runtime
   stamps the dispatch instant when the machine enters `AwaitingEcho`
   (initial dispatch and every re-type). A new interactor sweep (shape it
   after `reap_stale_spawns(now)` — injected clock, no wall-clock sleeps in
   tests) runs from the same periodic loop in
   `backend/crates/apps/delta-server/src/state.rs` that already drives the
   launch watchdog, and feeds `EchoDeadline` to any session whose
   `AwaitingEcho` age exceeds the deadline.
3. **Deadline constant, generous and overridable.** Default around
   `ECHO_DEADLINE = 60s` as a named constant with a doc comment: it must
   comfortably exceed the real echo loop's measured worst case (~15s under
   load: keystroke → tmux → transcript tail → hook) and leave headroom for
   auto-compact windows. Make it overridable via a `DELTA_*` env var
   (existing pattern in `backend/crates/apps/delta-server/src/main.rs`) so
   the fake e2e suite can use a short window.
4. **Recovery flows through the existing budget — no new machinery.** First
   deadline → `Requeue` → the send returns to `queued` and the normal idle
   flush re-dispatches it (if the swallowing state is gone, the echo matches
   and the send self-heals). Second deadline for the same send → the budget
   is spent → `park_unechoable_send` cancels the row and broadcasts the
   existing `SessionEvent::SendParked` with the text (the frontend already
   renders it). Anything queued behind promotes through the normal flush.
5. **Escape before a deadline-triggered re-type.** Before re-typing a
   send that was requeued *by the deadline path*, inject a single `Escape`
   into the pane — the same primitive the dispatched-send cancel path
   already uses — so a lingering modal is dismissed and a partially-landed
   composer draft is discarded (re-typing stays idempotent even in the
   "Enter swallowed but text landed" variant). Scope this to the
   deadline-requeue path; the compact re-dispatch and normal dispatch paths
   keep their current keystroke sequences.
6. **Prompt flush after the deadline transition.** After `EchoDeadline`
   lands the machine in `Idle`, run the queued-send dispatch flush in the
   same actor turn, so the requeued send re-types promptly instead of
   waiting for the next unrelated idle signal.

Operation × state coverage (deadline vs the states `AwaitingEcho` can meet):

- Keystrokes swallowed once (modal gone by the retry) → first deadline
  re-types with a leading Escape, echo matches, send `matched` — self-heal.
- Keystrokes swallowed twice → second deadline parks: row `cancelled`,
  `SendParked` broadcast with the text, next queued send dispatches.
- Echo arrives before the deadline → the stamp is cleared with the state
  transition; a sweep tick that raced it is a stale no-op (no double-type).
- A real settle input (`Stop`, `Interrupt`, `Close`, `Cancel`,
  `EchoMatched`, `ExternalPrompt`) lands first → `EchoDeadline` afterwards
  is a stale no-op.
- Send swallowed by auto-compact → the compact re-dispatch (which re-types
  while statuses stay `dispatched` and consumes no budget) remains the
  primary recovery; each re-type re-stamps the deadline. If a compact
  outlasts the deadline anyway, the watchdog costs at most the one budgeted
  retry and then parks with the text returned — never a silently stuck
  queue.
- Server restart while `AwaitingEcho` → unchanged: the boot reconcile
  already restores such rows to `queued` + `restored_at` awaiting explicit
  release.
- Adapter-backed (Codex) sessions → structurally unaffected: their sends
  match inside the turn-start call and never enter `AwaitingEcho`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The transition table covers `EchoDeadline`: matching-id in
      `AwaitingEcho` yields `Idle` + `Requeue(send_id)` without the anomaly
      flag; a mismatched id and every other state are no-ops — asserted as
      new rows in the existing exhaustive table test in `turn.rs`.
- [x] Interactor regression test reproducing the incident: a dispatched send
      that receives **no** subsequent inputs is requeued exactly once by the
      first deadline (re-typed with a leading `Escape` keystroke asserted on
      the tmux port), and parked by the second — asserting the `cancelled`
      row, the `SendParked` event carrying the original text, and that a
      send queued behind it is then dispatched.
- [x] Interactor test: when the echo matches before the deadline, a later
      sweep tick produces no requeue and no re-type (stale deadline is a
      no-op; normal sends are never double-typed).
- [x] The sweep is driven by an injected `now` like `reap_stale_spawns`
      (tests advance a synthetic clock; no wall-clock sleeps), and the
      deadline default is a named documented constant overridable via a
      `DELTA_*` env var.
- [x] fake-claude gains a scripted step that swallows one prompt without
      echoing (simulating a TUI modal), and an e2e-fake spec drives: send
      during the swallow window → parked notice rendered in the browser →
      a follow-up send flows normally (uses the shortened deadline via the
      env override).
- [x] Existing send/echo, slash-command, compact-redispatch, and
      requeue-budget tests pass unmodified, and no sqlite schema change
      (`SCHEMA_VERSION` untouched).
- [x] `docs/guides/api/sends.md` documents the deadline in the send
      lifecycle: `dispatched` with no echo → one deadline-driven retry →
      parked with `SendParked`, including the constant and its env
      override.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real Claude session driven by Delta: attach to the pane,
      open an interactive TUI dialog (e.g. the model selector) and leave it
      up, then send from the browser. Observe either self-heal (Escape +
      re-type after the first deadline, answered once) or, if the dialog is
      re-opened to swallow the retry too, the parked notice with the
      original text — and in both cases a subsequent send flows. The
      permanent "In progress / 1 queued" state can no longer be produced.
- [ ] A normal real-session send is delivered exactly once (no double-typing
      from a racing deadline) and the transcript shows a single prompt.

## Out of scope

- The structural unstick/attribution redesign (driving unstick from
  transcript turn boundaries and demoting text matching to
  attribution-only). This watchdog is deliberately its narrow last-resort
  layer and must not change the attribution rules.
- Pre-dispatch pane-state probing (detecting modals via capture-pane) —
  representation-dependent and unnecessary once the watchdog bounds the
  damage.
- Suppressing Claude Code's own onboarding prompts at spawn time.
