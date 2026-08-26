---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/attribution-split
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "fn hold_send_for_release" backend/crates/gateway/delta-sqlite/src/store && grep -q "fn hold_send_for_release" backend/crates/domain/delta-usecase/src/ports/session_store.rs && ! grep -q "follow-up for the attribution redesign" backend/crates/domain/delta-usecase/src/ports/session_event.rs && ! grep -q "Restored after restart" frontend/packages/apps/web/src/features/composer/PendingQueue.tsx && grep -q "held_at" backend/crates/gateway/delta-sqlite/src/migrations/send.rs && ! grep -rq "restored_at" backend/crates/domain backend/crates/apps backend/crates/gateway/delta-wire backend/crates/gateway/delta-sqlite/src/store frontend/packages/apps frontend/packages/gateway/api-client frontend/packages/gateway/wire-gen/src/generated docs/guides'
assignee: null
branch: task/0826-1515-feat-parked-send-stays-in-the-queue
created_at: 2026-08-26T15:15:08Z
updated_at: 2026-08-26T17:27:11Z
---

# feat(sends): keep a parked send in the queue for an explicit release instead of cancelling it

## Overview

When a dispatched send is never heard about again — its keystrokes were
swallowed by a TUI dialog, twice, so the echo deadline expired twice — Delta
*parks* it: `park_unechoable_send` in
`backend/crates/domain/delta-usecase/src/interactor/turn_input.rs` cancels the
row and broadcasts `SessionEvent::SendParked { text }`. The only copy of the
message that survives is the text inside that one event, rendered as a red
"not delivered" card (`TranscriptPane.tsx`, `send-parked-notice`) that tells
the user to copy it and try again — and that card lives in browser state only:
a reload, a second tab, or a session switch loses it. The doc on `SendParked`
in `ports/session_event.rs` says as much ("Making it recoverable is a follow-up
for the attribution redesign"). With consumption now positional the park path
is rare — it fires only when nothing at all reached the pane — but rare is
exactly when a silently lost message hurts most.

Delta already has the right shape for "a send that must wait for the user's
explicit go-ahead": the boot-time restore. `restore_all_dispatched` moves every
row that was `dispatched` when the previous process died back to `queued`
**with the `restored_at` marker**, the queued-dispatch selection skips marked
rows so they never auto-send, the open-send list shows them with a Send action
that calls `POST /api/sends/{id}/release` (`release_restored_send` clears the
marker) and the usual Cancel, and `docs/guides/api/sends.md` documents the
marker. Parking should use the same mechanism: the row goes back to `queued`
with the marker, stays visible in the queue, and the user decides — release it
(re-typed on the next idle, once) or cancel it. The marker is renamed to say
what it now means — see item 1.

### What changes

1. **Store / port.** Add `SessionStore::hold_send_for_release(id) -> Result<bool>`
   implemented in `delta-sqlite/src/store/sends.rs` as
   `UPDATE send SET status = 'queued', restored_at = ?now WHERE id = ?1 AND status = 'dispatched'`
   (returning whether a row changed), mirroring `restore_all_dispatched`'s
   shape for one row; add it to the usecase `fake_store`. The marker now has
   two producers (the boot restore and the park) and one meaning — "held in
   the queue until the user releases it" — so **rename it to `held_at`**
   everywhere: a migration step (`SCHEMA_VERSION` 5 → 6,
   `ALTER TABLE send RENAME COLUMN restored_at TO held_at`, documented in
   `migrations/send.rs`'s module doc where the column is described), the
   `delta-model` `Send` field, the port (`release_restored_send` →
   `release_held_send`; `restore_all_dispatched` keeps its name — it names
   the boot action, not the marker), the sqlite store and its tests, the
   fake store, the usecase (`release_send.rs`), the wire type (`WireSend`,
   then `make gen`), the api-client, the web app (`PendingQueue`, stores,
   tests, e2e-fake specs and scenarios), and the three API guides. Wire
   renames are unrestricted during `v0.x` (`docs/guides/compatibility.md`),
   so no annotation is needed; the release-notes entry is the commit subject.
   `SendNotReleasable` and the release endpoint path stay.
2. **Park path.** `park_unechoable_send` calls `hold_send_for_release` instead
   of `cancel_send`; it still forgets the requeue budget entry and still
   broadcasts `SendParked { text }` (the notice is how the user learns *why*
   the row is waiting). If the guarded update affects no row (the send is no
   longer `dispatched`), log and skip the event as today's read-back branch
   does. Update the `SendParked` doc in `ports/session_event.rs` — remove the
   "follow-up" sentence and describe the row's new state — and the parked
   branch of `docs/guides/api/sends.md` (~line 291, "Second deadline — parked")
   plus the restored-row section so the marker's two producers are documented
   in one place. Update `unmatchable_send_is_never_redispatched` /
   `swallowed_send_is_retyped_then_parked_by_the_echo_deadline` to assert the
   row ends `queued` with `restored_at` set (not `cancelled`), and add a
   sqlite test for the guarded update (`store/tests/sends.rs`).
3. **Frontend.** `PendingQueue.tsx` renders marked rows with the label
   "Restored after restart"; that is now wrong for a parked row. Use one
   neutral label for both producers (e.g. "Held — send or cancel") and keep
   the existing Send / Cancel actions; update `PendingQueue.test.tsx` and any
   e2e spec that asserts the old label (grep for it). Reword the
   `send-parked-notice` card: the message is back in the queue, choose Send or
   Cancel — it must no longer say "copy it and try again" and must no longer
   be the only place the text lives (drop the scrolled text block if the queue
   entry now shows it; keep the card short). Keep `data-testid` values so the
   e2e-fake specs that cover the notice still find it; update their
   assertions to the new state (row present in the queue with the marker,
   Send releases it once).
4. **Docs left over from the positional changes on this branch** (same PR,
   doc-only): `docs/guides/api/sends.md` does not yet describe how a
   slash-command send is resolved (its command line or unknown-command notice
   consumes it and ends the turn — no echo, no `Stop`); add a short paragraph
   next to the echo-deadline section. `SessionStore::head_dispatched_send`'s
   doc in `ports/session_store.rs` says the hook consumes the send because its
   keystrokes are already in the pane; add the resume-window exception (held,
   not typed, so a prompt then is not its echo).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `SessionStore::hold_send_for_release` exists in the port and the sqlite
      store with a `status = 'dispatched'` guard (gates appended to
      `check_command`; the guard is pinned by a sqlite test).
- [x] A send whose echo deadline expires twice ends `queued` with `restored_at`
      set, is not auto-dispatched on the next idle, and `SendParked` is still
      broadcast — pinned by the updated
      `swallowed_send_is_retyped_then_parked_by_the_echo_deadline`.
- [x] Releasing a parked row through the release endpoint clears the marker and
      it dispatches once on the next idle — pinned by a usecase test (reuse the
      release-endpoint test pattern for boot-restored rows).
- [x] `PendingQueue` shows a marked row with the neutral label and Send /
      Cancel, for both a boot-restored and a parked row; the old label is gone
      (gate appended to `check_command`) — pinned by `PendingQueue.test.tsx`.
- [x] The `SendParked` doc no longer defers recoverability to a follow-up (gate
      appended to `check_command`).
- [x] The marker is `held_at` end to end: a migration step renames the column
      (`SCHEMA_VERSION` 6, pinned by the migration tests), and `restored_at`
      no longer appears in the domain/app/wire/store code, the web app, the
      api-client, the generated bindings, or the API guides — only in the
      migration history (gates appended to `check_command`).
- [x] `make check` is green (backend, generated-bindings freshness — no wire
      change expected, frontend, both Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [x] On a real Claude Code session, send `/help` from Delta: the built-in's
      command picker swallows the submit, and swallows the retry too, so after
      the echo deadline fires twice (about two minutes) the send reappears in
      the queue with the neutral label and the parked notice, and Cancel removes
      it. (`/help` is used precisely because it keeps swallowing — a dialog
      opened by hand is closed by the first deadline's Escape, so the retry
      goes through and the send never parks. Press Escape in the pane
      afterwards to dismiss the picker. Releasing a parked row is covered by
      the automated release tests; `/help` cannot demonstrate it because a
      re-typed `/help` is swallowed again.) Verified 2026-08-27: after the
      restart the migration stamped version 6 and left `delta.db.bak-v5`; the
      `/help` send hit the deadline twice (60 s apart), was parked as a held
      row with the notice, and Cancel removed both; the pane's picker was
      dismissed with Escape afterwards.

## Out of scope

- Any browser notice for a rewritten echo (the rewritten line is already
  attributed to the send's thread and visible there; a server warning
  suffices).
- The turn machine and the Codex adapter.
