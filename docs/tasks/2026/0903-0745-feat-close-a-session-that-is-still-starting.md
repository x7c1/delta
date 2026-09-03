---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/close_session_cancels_a_pending_spawn_and_reports_spawn_failed.rs && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/close_session_cancels_a_launch_still_preparing.rs && grep -q "still starting" docs/guides/api/sessions.md && ! grep -q "unbound spawn is not a supported operation" frontend/packages/apps/web/src/features/navigator/SessionNode.tsx && grep -q "cancelled" backend/crates/domain/delta-usecase/src/ports/session_event.rs && grep -q "Launch cancelled" frontend/packages/apps/web/src/features/composer/PendingQueue.tsx'
assignee: null
branch: task/0903-0745-feat-close-a-session-that-is-still-starting
created_at: 2026-09-03T07:45:00Z
updated_at: 2026-09-03T16:56:45Z
---

# feat(sessions): close a session that is still starting

## Overview

A session that is still starting (`status: "spawning"`, the amber `Starting`
badge) offers the user no way out. The navigator hides `Close` for it
(`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`, the
`item.open && !spawning` guard on the menu items, with a comment saying
closing an unbound spawn is unsupported), the composer is disabled while it
spawns, and the pending chip's Cancel does not stop the launch. On a healthy
launch the state lasts seconds, so this was never pressing. But when a launch
wedges — most recently a hook regression left a session `spawning` for good,
with a live pane nobody could reach from Delta — the only remedies are a
server restart or a manual database edit. Delta must let the user close a
starting session, whatever stage its launch is in.

### Semantics: closing a starting session cancels its launch

A session that has not bound holds no conversation data yet — its row was
written eagerly at accept time (`insert_spawning_session`) and no transcript
line has been ingested — so "closing" it is not the tear-down-but-keep of a
bound session; it is the same outcome the launch watchdogs already produce
when a launch never comes up. Reuse that outcome rather than inventing a
fourth end state:

- tear down whatever the launch has stood up so far,
- drop the turn entry (`forget_turn`),
- read the undelivered sends (`undelivered_sends`, **before** the rows go),
- clean up the eager row (`clean_up_failed_spawn_row`: deleted when it has
  no messages, `failed` otherwise),
- report a `SessionEvent::SpawnFailed` carrying `unsent` and a `reason` that
  says the user closed it (e.g. `closed while starting`), so the browser's
  existing `spawn_failed` handling does the rest: the spawn chip flips to
  Retry / Dismiss with the reason, the unsent text is restored into the
  new-session composer draft, focus is handed back to the new-session screen,
  and the session list is refetched without the row
  (`frontend/packages/apps/web/src/data/applySessionEvent.ts`,
  `store/live/spawnsSlice.ts`).

The launch can be in one of three runtime shapes when the close arrives
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/`),
and `SessionContext::close_session`
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/close_session.rs`)
must handle each. Today it handles none — its doc comment says a starting
session is a no-op on the pane side, which is what leaves the user stuck:

1. **Launching** (`LaunchingSpawn`: accepted, preparation still running —
   worktree build, trust seed, tmux, or for Codex `connect` + `thread/start`).
   Take the launching entry (`take_launching_for_token` on the entry's own
   token, or add a `take_launching()` that does not need the token) and run
   the same rollback as a failed preparation
   (`finish_launch.rs::roll_back_failed_launch`, today private to that
   module — widen it or extract the shared body) with `pane_token` `Some`
   for a `LaunchTarget::Pane` and `None` for an adapter target. The
   background task keeps running, and that is already safe: when it posts
   `LaunchPrepared` / `AdapterLaunchPrepared` the handlers find no launching
   entry and answer `LaunchApproval::Abandon` (`record_launched_pane.rs`,
   `adapter_launch.rs`), so no pane is created and a connected adapter is
   dropped; its `LaunchFinished` then finds nothing to settle and is logged
   and ignored. Update those doc comments where they enumerate the reasons an
   entry can be missing ("the acceptance was already rolled back") to name
   the explicit close as well. A worktree the build was in the middle of
   creating may still be created — accept that, exactly as the preparation
   deadline does.
2. **Pending** (`PendingSpawn`: the pane exists, no hook has bound it).
   Take the pending entry (`take_unbound_pending`), kill the pane best-effort
   (`kill_pane_best_effort`), and run the same rollback. This is the spawn
   half of `reap_stale_spawns.rs::reap_stale_launch` with a reason instead
   of `None`; factor the shared body so the watchdog and the close call one
   helper rather than two copies.
3. **Bound, but the row is still `spawning`** (defensive: a bind whose row
   activation failed — the hook fix that keeps a spawn pending until its
   registration succeeds makes this unreachable from the hook path, but rows
   in that state can exist from before it and nothing else repairs them).
   The existing close path already kills the pane and drops the binding; add
   that when the stored row's `status` is `Spawning` after the tear-down,
   the row is cleaned up and `SpawnFailed` reported exactly as above, so the
   card does not stay amber forever with nothing left behind it.

For all three, `close_session` keeps its current signature (`Result<Vec<SessionEvent>>`);
return the `SpawnFailed` in that vector so the HTTP handler
(`backend/crates/apps/delta-server/src/api/mod.rs::close_session`) broadcasts
it the way it broadcasts the subagent sweep's events today. The handler also
broadcasts `session_closed` unconditionally; keep that (the browser only
invalidates the list on it, and the row being gone is fine) unless it proves
to confuse the client, in which case suppress it when the close cancelled a
launch and say so in the API doc. A `spawning` session's close still answers
`204`. A closed-but-known session and an unknown id keep today's behaviour
(no-op and `404`).

Rewrite the `close_session` doc comment: the paragraph that describes a
starting session as a no-op is now wrong, and the operation has two outcomes
(tear down and keep, or cancel and remove) that the comment should state
plainly.

### Frontend

- `SessionNode.tsx`: offer `Close` for a `spawning` card too (drop the
  `!spawning` half of the guard and the comment explaining why it was
  hidden; the `item.open` half stays so a closed card still hides it). The
  mutation is unchanged — `POST /api/sessions/{id}/close` — and the
  `spawn_failed` that follows already removes the card and shows the
  Retry / Dismiss chip, so no new client state is needed. Add a
  `SessionNode.test.tsx` case next to `exposes both "Copy session ID" and
  "Close" while the session is open`: a `spawning` item exposes `Close`, and
  selecting it posts to the close endpoint. Check `WorkspaceScreen.tsx` /
  `TranscriptPane.tsx` for any second `Close` affordance gated on
  `open && !spawning` and treat it the same way.
- MSW handler (`frontend/packages/testing/api-mocks/src/handlers.ts`,
  `POST */api/sessions/:id/close`): mirror the server — for an entry whose
  `session.status === 'spawning'`, remove it from `store.sessions` and emit a
  `spawn_failed` for it (with the reason and its open sends as `unsent`) on
  the mock event channel the handlers already use for lifecycle events;
  otherwise flip `open` as today. Add a handler test.

### Documentation

- `docs/guides/api/sessions.md`, `POST /api/sessions/{id}/close`: describe
  the two outcomes. Use the phrase "still starting" for the spawning case
  (grep gate in `check_command`): the launch is cancelled, the row is
  removed, `spawn_failed` carries the reason and the undelivered sends, and
  `204` is still the answer.
- `docs/guides/api/live-channels.md`, `spawn_failed`: the producer list says
  "Three producers"; add the explicit close of a still-starting session as
  the fourth, and update the same enumeration in the
  `SessionEvent::SpawnFailed` doc comment
  (`backend/crates/domain/delta-usecase/src/ports/session_event.rs`) and in
  the `spawn_failed` notes of `docs/guides/api/sends.md` if they enumerate
  producers.

### Tests (backend)

One test per file under
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/`, as the
siblings do. Harness pointers: `close_session_kills_the_pane_and_keeps_the_data.rs`
(spawn, bind, close), `reap_stale_spawns_reaps_an_expired_unbound_spawn.rs`
(a pending spawn and what its reap asserts),
`a_launch_preparation_that_outruns_its_deadline_reports_spawn_failed.rs`
(holding a preparation open with `WorktreeGate::closed()` and reading
`SpawnFailed` off the event sink), and
`a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed.rs` (the
adapter-backed launch harness).

- `close_session_cancels_a_pending_spawn_and_reports_spawn_failed` — spawn
  with a first prompt (no bind), close: the pane token is in the fake tmux's
  `killed`, the pending id set is empty, the row is gone from the store, and
  the returned events hold one `SpawnFailed` for the id whose `reason`
  mentions the close and whose `unsent` carries the first prompt's text.
- `close_session_cancels_a_launch_still_preparing` — hold the worktree gate
  closed, accept a new session, close it while the build is parked: the row
  is gone and `SpawnFailed` is reported; then open the gate and let the task
  finish — assert the fake tmux recorded **no** `create_session` (the
  `LaunchPrepared` answer was `Abandon`) and the store still has no row.
- A Codex variant of the preparing case if the adapter harness makes it
  cheap: close during the adapter launch, assert the row is gone and that
  the adapter's connection was dropped rather than bound.
- `close_session_on_a_bound_session_whose_row_is_still_spawning_cleans_it_up`
  if the fake store lets the test leave a bound session's row at `spawning`
  (it may need a fake-store knob; add one only if small). Otherwise name the
  shape as untested in the task body's out-of-scope note and cover it by the
  code path's own comment.
- Keep `close_session_known_but_not_open_is_a_noop` and
  `close_session_unknown_id_is_session_not_found` green as they are: the
  no-op and `404` behaviours do not change.

### Session-state coverage

The operation changed is **Close**. States and outcomes:

- **closed** — no-op, `204` (unchanged, existing test).
- **open + idle** / **open + mid-turn** — tear down, keep data (unchanged;
  mid-turn's send handling via `TurnInput::Close` is unchanged).
- **resuming** — unchanged: the pane is bound, so the existing path kills it
  and `TurnInput::Close` cancels the held prompt.
- **spawning: launching** — cancel and remove (new test).
- **spawning: pending** — cancel and remove (new test).
- **spawning: bound with an unactivated row** — tear down, then remove
  (defensive path; test if the harness allows).
- **unknown id** — `404` (unchanged, existing test).

### Pipeline notes

- Backend and frontend both change; `make check` covers both. Wire types are
  unchanged (`spawn_failed` already carries `reason` and `unsent`), so
  `make gen` should produce no diff — if it does, stop and report rather
  than committing generated churn.
- The appended gates fail on `main`: the two test files do not exist, the
  API doc does not say "still starting", and the navigator still carries the
  "not a supported operation" comment.

### Presenting the cancel (decided during refine)

Reusing `spawn_failed` verbatim made a user-requested cancel look like a
failure in the browser (a danger-toned "The session failed to start" row, or
an error snackbar in a tab that did not start the spawn), and the event gave
a client no way to tell the two apart except by matching the prose `reason`.
Decision: add a boolean `cancelled` to `spawn_failed` (`true` only for the
explicit close; `false` for the three failure producers), regenerate the wire
types, and let the browser present a cancelled launch neutrally — chip text
"Launch cancelled. Retry or dismiss it." with a non-danger tone, an
informational rather than error snackbar in the untracked-tab path, and a
short informational notice when the cancelled session was not the focused one
(its unsent text is still returned to the new-session draft, so the user is
told where it went). The `Close` label stays; the `reason` stays
`closed while starting`. The MSW mock and the API docs (`live-channels.md`
frame + field, `sessions.md`) follow.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Closing a session whose pane is up but unbound kills the pane, removes
      the eager row, and returns a `SpawnFailed` carrying the undelivered
      first prompt and a reason naming the close (test file
      `close_session_cancels_a_pending_spawn_and_reports_spawn_failed.rs`;
      `test -f` gate).
- [x] Closing a session whose launch preparation is still running removes
      the row and reports `SpawnFailed`, and the preparation's later
      checkpoint is abandoned so no pane is created (test file
      `close_session_cancels_a_launch_still_preparing.rs`; `test -f` gate).
- [x] Closing a closed-but-known session is still a no-op and an unknown id
      is still `404` (existing lifecycle tests stay green under `make check`).
- [x] The navigator offers `Close` on a `spawning` card and the old
      "unsupported" comment is gone (`SessionNode.test.tsx` case; negative
      grep gate on the comment text).
- [x] The MSW close handler removes a `spawning` entry and emits
      `spawn_failed` for it (handler test under `make check`).
- [x] `docs/guides/api/sessions.md` documents the still-starting outcome of
      `POST /api/sessions/{id}/close` (grep gate for "still starting").
- [x] `spawn_failed` carries `cancelled` (`true` only for the explicit close)
      and the browser presents a cancelled launch without failure wording
      (grep gates on `session_event.rs` for `cancelled` and on
      `PendingQueue.tsx` for "Launch cancelled"; the backend tests assert the
      flag per producer).

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, start a new session on a branch whose worktree
      fetch takes a few seconds and choose `Close` on the amber card while
      it still reads `Starting`: the card disappears, the new-session
      composer shows the Retry / Dismiss chip with the close reason and the
      original prompt restored in the draft, and no stray `claude` pane is
      left in `tmux -L delta ls`.
- [ ] A session that was wedged `spawning` before this change (a bound pane
      with an unactivated row) can be closed from its card and leaves the
      list. (Non-blocking for merge under the CI-green autonomous policy;
      recorded for dogfooding.)

## Out of scope

- Editing or cancelling individual `queued` sends of a starting session; the
  close cancels the whole launch.
- Removing the worktree a cancelled launch may have created; worktree
  cleanup on close is deferred for bound sessions too (see the existing
  comment in `close_session`).
- A separate "Cancel" wording or endpoint for the starting case. The
  navigator keeps the single `Close` item; if the user-experience review
  finds the label misleading on a starting card, changing the label text is
  in scope, a new endpoint is not.
- Any change to the launch deadlines (`LAUNCH_PREP_DEADLINE`,
  `PENDING_SPAWN_DEADLINE`).
