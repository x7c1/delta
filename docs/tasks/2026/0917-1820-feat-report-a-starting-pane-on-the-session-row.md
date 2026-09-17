---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rq 'startingPanes' frontend/packages/apps/web/src/"
assignee: null
branch: task/0917-1820-feat-report-a-starting-pane-on-the-session-row
created_at: 2026-09-17T09:20:41Z
updated_at: 2026-09-17T10:40:07Z
---

# feat(session): report a starting pane on the session row, so a reload can attach to it

## Overview

A launch that has not bound yet can already have its tmux pane up, and the
embedded terminal can attach to it — that is how a user answers Claude
Code's trust dialog. The browser learns the pane is up from exactly one
place: the one-shot `spawn_pane_ready` event, kept in the in-memory
`startingPanes` set (`frontend/packages/apps/web/src/store/live/startingPanesSlice.ts`).
`WorkspaceScreen.tsx` reads it to resolve `terminalPaneState` to `'starting'`
(attachable) rather than `'preparing'` (not yet).

A browser that was not listening when the event fired never finds out:

- **Reload mid-launch.** The set is gone, the session row still reads
  `spawning`, and the terminal shows its "preparing" note until the launch
  binds or fails — which, for a launch stuck on the trust dialog, is never.
  The one window the user needs is the one a reload takes away.
- **A second tab** opened after the event is in the same position.

The slice's own doc comment accepts this ("a browser that missed the event
… just does not offer the terminal until the session binds"). It should not
have to.

### Change

Make the session row the single source of this fact, the way it already is
for `open`.

- **Backend.** `SessionListing`
  (`backend/crates/domain/delta-usecase`, built by `listing_for` in
  `interactor/listing/list_sessions_page.rs`) gains `pane_starting: bool`:
  the session's actor holds an attachable pane that is **not bound** —
  `SessionRuntime::attachable_pane()` returns `Some` with `bound: false`
  (`session_actor/runtime/pane_attach.rs`). That is the same predicate the
  `/pty` bridge resolves against, so the row says "attachable" exactly when
  an attach would succeed. Note that `has_live_pane()` / `QueryIsLive` is
  **not** this fact: it is also true while the launch is still preparing and
  no pane exists yet.
  `listing_for` already asks the actor for `open` per row. Do not add a
  second actor round-trip per row next to it: answer both facts from one
  query (for example a small struct reply replacing the `QueryIsOpen` use
  here), so the two are a consistent snapshot. A session with no actor has
  no starting pane by definition.
- **Wire.** `WireSessionListItem`
  (`backend/crates/gateway/delta-wire/src/rest/sessions_response.rs`) carries
  `pane_starting`; regenerate the TypeScript bindings (`make gen`).
- **Frontend.** `WorkspaceScreen.tsx` reads `pane_starting` off the focused
  row instead of `paneIsStarting(startingPanes, …)`. `spawn_pane_ready`
  joins the lifecycle events that invalidate the session list in
  `data/applySessionEvent.ts` (`session_registered` / `session_opened` /
  `session_closed` already do), so the row refreshes when the pane comes up.
  With that, the `startingPanes` slice, its reducers and their wiring in
  `liveStore` have no reader left: **remove them** rather than keep two
  sources of one fact. Update the mock API (`frontend/packages/testing/api-mocks`
  and any e2e REST helper that builds a session list item) for the new
  field.
- The `spawn_pane_ready` event itself stays on the wire — it is what tells a
  listening browser to refetch. Re-sending it over the WebSocket on
  reconnect was considered and rejected: the list is refetched on reconnect
  anyway, and a replayed one-shot event needs bookkeeping the row does not.

### Tests

- usecase: a launched-but-unbound spawn lists with `pane_starting: true` and
  `open: false`; once bound it lists `open: true`, `pane_starting: false`;
  a spawn accepted but still preparing (no pane yet) lists
  `pane_starting: false`; a failed launch and a closed session list
  `pane_starting: false`.
- wire: the JSON shape test in `sessions_response.rs` includes the field.
- frontend: `WorkspaceScreen` resolves `'starting'` from a row with
  `pane_starting: true` on first render with no event ever delivered (the
  reload case), and `applySessionEvent` invalidates the session list on
  `spawn_pane_ready`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `GET /api/sessions` reports `pane_starting` per row, true exactly
      while the actor's `attachable_pane()` is unbound, covered by usecase
      tests for the five states above (preparing, pane up, bound, failed,
      closed).
- [x] `open` and `pane_starting` for one row come from a single actor
      query.
- [x] The generated TypeScript bindings carry the field (`make check` runs
      `gen-check`).
- [x] A `WorkspaceScreen` test renders a focused `spawning` row with
      `pane_starting: true` and no `spawn_pane_ready` event, and the
      terminal pane is in its attachable state.
- [x] `applySessionEvent` invalidates the session list on
      `spawn_pane_ready` (test).
- [x] The `startingPanes` slice is gone: `check_command` greps that
      `startingPanes` no longer appears under the web app's `src/`.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real server: start a session in a directory Claude Code has
      not been trusted in, reload the browser while the trust dialog is up,
      and confirm the terminal attaches and the dialog can be answered.
