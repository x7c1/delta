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
branch: task/0813-0826-fix-permission-approval-queue
created_at: 2026-08-13T08:26:52Z
updated_at: 2026-08-13T10:54:00Z
---

# fix(permission): queue concurrent approval requests instead of last-writer-wins single slot

## Overview

The session runtime tracks the pending tool-permission dialog in a single
slot, and a new request silently replaces the previous one
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/permission.rs`
~line 52: "a new dialog replaces a stale one — `claude` shows one at a
time"). That invariant is Claude-shaped: the pane-backed hook blocks Claude
Code serially, so at most one request is ever outstanding. An adapter-backed
provider breaks it — Codex runs parallel tool calls, and a single turn can
emit N `item/commandExecution/requestApproval` server requests in the same
millisecond.

Observed in dogfooding (2026-08-13, a real `codex app-server` session): one
turn fanned out 12 escalated `exec_command` calls via `Promise.all`. All 12
approval requests were persisted as `permission_request` rows and all 12
row-id ↔ provider-token correlations were kept (the token side is already a
`HashMap`,
`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/agent.rs`
~line 17), but each `PermissionRequested` event overwrote the runtime
mirror, so the sends envelope and the browser dialog only ever showed the
last request. The user's single Allow answered wire id 12; the other 11
requests were never answered, Codex waited on them forever, the turn stayed
`in_flight`, and the envelope's `permission` field went back to `null` — a
deadlock invisible to the user ("Codex never responds").

Fix: make the pending-permission mirror an ordered FIFO queue keyed by
request id, end to end. Implementation direction (details are yours,
invariants are not):

- **Runtime queue.** Replace `pending_permission: Option<PendingPermission>`
  with an ordered collection. Enqueue on `PermissionRequested` (never
  overwrite; the reducer is
  `backend/crates/domain/delta-usecase/src/interactor/agent_permission.rs`
  ~line 60), remove by key on `PermissionResolved` (a resolution for a
  non-head id removes only that entry and leaves the head alone), clear the
  whole queue where the single slot is cleared today (turn returns to idle,
  session closed). The Claude path needs no behavior change — its hook
  serialization means the queue length never exceeds 1 there.
- **Decisions stay keyed by row id, not by queue position.** The decision
  path (`backend/crates/domain/delta-usecase/src/interactor/permission_decision.rs`)
  already resolves any row id — Claude via the waiter map, Codex via the
  token map. Keep that: the queue orders what the browser sees; it must not
  gate which request `POST /api/permissions/{id}/decision` can answer.
- **The envelope reports the head plus the depth.** The sends envelope
  (`backend/crates/gateway/delta-wire/src/rest/sends_response.rs` ~line 168)
  keeps `permission` as the queue head (shape unchanged) and adds a count of
  pending requests (head included), so a reconnecting client can rebuild both
  the dialog and a "N approvals pending" indication from a plain refetch.
  Update `docs/guides/api/sends.md` to describe the queue semantics and the
  new field.
- **The browser is never left dialog-less while requests are pending.** The
  invariant that failed in the field: after the user answers the visible
  dialog, the next pending request must surface without a manual refresh.
  Suggested mechanism: when a resolution removes the head and the queue is
  non-empty, re-emit `SessionEvent::PermissionRequested` for the new head
  (the frontend notice slice,
  `frontend/packages/apps/web/src/store/live/noticesSlice.ts`, already keys
  removal by request id, so a resolve-then-requested sequence keeps a dialog
  visible). Any equivalent mechanism is fine as long as the invariant is
  tested.
- **Frontend shows the head, not the last writer.** Today N rapid
  `permission_requested` events leave the notice showing the Nth request.
  With the queue, the notice must show the head and surface the remaining
  count (e.g. "+11 more"), and answering must walk the queue front to back.
- **fake-codex fidelity.** The scenario DSL
  (`backend/crates/apps/fake-codex/src/scenario.rs`) can emit approval
  requests, but has no way to model the real behavior observed in the field:
  N requests outstanding at once, with the turn completing only after all N
  are answered. Extend the fake so a scenario can emit several
  `request_approval` steps without waiting in between and then suspend the
  turn until every outstanding approval has been answered, and use it for the
  coverage below.

Operation × state coverage (permission decision / approval arrival vs queue
state — write a test per row):

- Request arrives, queue empty → it becomes the head; envelope shows it,
  count 1.
- Request arrives, queue non-empty → enqueued behind the head; head
  unchanged; count increments.
- Head resolved, queue has more → next entry promotes to head; the browser
  gets a dialog for it without refetching; count decrements.
- Non-head entry resolved (decision keyed to a row that is not the head) →
  only that entry leaves the queue; head unchanged.
- One of N denied → only that request is declined on the wire; the rest stay
  pending and answerable.
- Turn returns to idle / session closes with a non-empty queue → queue
  cleared exactly as the single slot is today (the provider has already
  settled or abandoned its requests by then; Delta only drops the mirror).
- Reconnect with N pending → envelope refetch reseeds the head dialog and
  the count.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Runtime unit tests: N `PermissionRequested` events queue in FIFO order
      (no overwrite); resolving the head promotes the next; resolving a
      non-head id removes only that entry; idle/close clears the queue.
- [x] Server-level test against fake-codex: a single turn emits ≥3 approval
      requests before any answer; all rows persist as `pending`; the sends
      envelope reports the head and the pending count; answering every
      request via `POST /api/permissions/{id}/decision` lets the turn
      complete; no row is left `pending` (the field failure — 11 orphaned
      rows — is the regression this pins).
- [x] A test asserts the no-dialog-less invariant: resolving the head while
      the queue is non-empty produces a client-visible signal for the new
      head (e.g. a re-emitted `permission_requested`), so a client that only
      follows events always has a dialog while requests are pending.
- [x] Frontend notice unit tests: N rapid `permission_requested` events show
      the first request (head), not the last; `permission_resolved` for the
      head followed by the promotion signal shows the next; the envelope
      seed path restores head + count after reconnect.
- [x] e2e (fake provider): a parallel-approval scenario drives the UI
      through answering all requests one after another until the turn
      completes, with the remaining-count indication visible while the queue
      is non-empty; existing Claude permission e2e passes unchanged.
- [x] `docs/guides/api/sends.md` documents the queue semantics (head,
      count, promotion) for the sends envelope and the decision endpoint;
      generated TS wire types are in sync (hash-compare gate in
      `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [x] Against a real `codex app-server` session: a prompt that triggers a
      parallel escalated `exec_command` fan-out (e.g. fetching many files at
      once with escalated permissions) surfaces every approval in the UI one
      after another; answering them all lets the turn complete, and the
      comms pane shows a response frame for every approval request id — no
      orphaned `pending` rows afterwards.
- [x] Run `make e2e-real-codex` (ignored canaries) once and confirm green —
      these do not run in CI.

## Out of scope

- Bulk approve/deny UI (answering the queue one dialog at a time is
  acceptable for this fix; a batch affordance is a separate UX task).
- Queueing `AskUserQuestion` dialogs (`question` stays a single slot;
  questions share the row-id space but Claude emits them serially).
- Extended Codex decision variants (`acceptWithExecpolicyAmendment`,
  `acceptForSession`, …) — v1 keeps the binary allow/deny mapping.
- Server-authoritative notification replay (the known Phase 5 gap for
  events lost while disconnected) — the envelope reseed added here narrows
  it for permissions but the general mechanism stays future work.
