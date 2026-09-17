---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && [ \"$(grep -rn '\\.deny_pending_permission_requests(' backend/crates/domain/delta-usecase/src/interactor --include='*.rs' | grep -v -E '/tests/|/testing/' | wc -l)\" -eq 1 ]"
assignee: null
branch: task/0917-2110-fix-settle-pending-permission-requests-when-a-bound-session-is-torn-down
created_at: 2026-09-17T12:10:08Z
updated_at: 2026-09-17T13:24:24Z
---

# fix(session): settle pending permission requests when a bound session is torn down

## Overview

When a pane-backed (Claude) session is torn down — the user presses Close,
or the background liveness sweep finds its pane gone — the shared teardown
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/tear_down_bound_session.rs`)
syncs the transcript, drops the binding, feeds `TurnInput::Close` and sweeps
running background subagents. It does not touch **pending permission
requests**. Their rows stay `pending`, the runtime's queryable queue
(`SessionRuntime` pending permissions) keeps them, and no
`permission_resolved` is broadcast.

What the user then sees:

1. `session_closed` arrives; the browser's notice lifecycle clears the
   permission dialog.
2. The same event invalidates that session's sends query. The refetched
   envelope still reports the pending request (it is built from the runtime
   queue), and the browser re-seeds the dialog — now over a **closed**
   session.
3. Pressing Allow answers 409 (the hook is no longer waiting), and the
   fallback text tells the user to answer in the embedded terminal, which
   reads "This session is closed". The only way out is Dismiss.

With the explicit Close there is at least a person who pressed it. Since the
pane-gone close, this also happens with nobody pressing anything: the agent
is killed while a dialog is up, and a tick later the dialog is back on a
closed session.

The adapter-backed (Codex) path already does the right thing when its
process dies: `settle_pending_permissions_on_death` in
`interactor/agent_event.rs` denies the rows with a reason, empties the
runtime queue in one step (so no successive head is promoted and
re-broadcast), drops the decision-routing entries (so a racing decision POST
answers 409 rather than 500), and broadcasts one `PermissionResolved` per
request through the shared reducer.

### Change

- Make the bound-session teardown settle pending permission requests the
  same way, for both of its callers (explicit close and pane-gone close): a
  request that can no longer be answered is denied, with a reason that says
  the session was closed, and announced as resolved. Share the settle
  routine with the adapter path rather than copying it — it is the same
  operation on the same state; only the reason text differs (the adapter
  path's is `PERMISSION_DENIED_ON_SESSION_DEATH`). Keep the ordering rule it
  documents: empty the queue before the per-request broadcasts.
- Event order for a teardown becomes: the permission resolutions, any
  `subagent_finished`, then `session_closed` (or `spawn_failed` where the
  explicit close reports that instead). Keep whatever order the two existing
  paths need internally, but nothing may follow `session_closed` for the
  session.
- Check the **pending question** (`question_asked` / the runtime's pending
  question) for the same shape. If a torn-down session leaves a question
  the browser re-seeds and cannot answer, settle it in the same change, the
  way the adapter path or the existing cancel path does; if it does not,
  say so in a comment where the permission settle is called and leave it.
- Out of scope, deliberately: a send still awaiting its echo is cancelled
  without an event on close. That is a separate decision about what the
  user should be told.
- Update `docs/guides/api/live-channels.md` and `docs/guides/api/sessions.md`
  where they state which events a close emits — they currently say the
  pane-gone close announces itself with `session_closed` alone (plus
  `subagent_finished`) and "without the per-request events".

### Tests

Usecase tests, one per caller × state:

- explicit close with one pending permission request → row denied with the
  closed reason, runtime queue empty, one `PermissionResolved` before
  `SessionClosed`;
- explicit close with two pending requests → both resolved, and the second
  is **not** re-broadcast as a fresh `PermissionRequested` in between;
- pane-gone close with a pending request → same as the first case;
- either close with no pending request → event list unchanged from today;
- a decision POST racing the close → `permission_not_pending`, not an
  internal error;
- after the teardown, the sends envelope for the session reports no pending
  permission.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Both the explicit close and the pane-gone close deny every pending
      permission request of the session and broadcast one
      `PermissionResolved` each before `SessionClosed` (usecase tests).
- [x] With several pending requests, none is re-announced as a new dialog
      during the settle (usecase test).
- [x] A decision arriving after the teardown answers "not pending" rather
      than an internal error (usecase test).
- [x] After the teardown the session's live state reports no pending
      permission (usecase test).
- [x] The adapter-death path and the pane-backed teardown share one settle
      routine: `check_command` greps that
      `deny_pending_permission_requests` is called from exactly one place
      under `interactor/` outside its tests and test helpers (a copy of the
      settle would make it two).

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real server: get a permission dialog on screen, kill the
      session's tmux session from outside, and confirm the dialog goes away
      and does not come back once the session shows closed.
