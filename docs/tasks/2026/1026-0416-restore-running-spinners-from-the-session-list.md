---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq 'fn list_sessions_page_reports_the_turn_in_flight' backend/crates/domain/delta-usecase/src/"
assignee: null
branch: task/1026-0416-restore-running-spinners-from-the-session-list
created_at: 2026-10-26T04:16:44Z
updated_at: 2026-10-26T04:59:15Z
---

# fix: restore every session's running spinner after a reload or reconnect

## Overview

The navigator's running spinner on a session card
(`data-testid="session-running"`) is held in the live store's
`runningThreads` (`frontend/packages/apps/web/src/store/live/runningThreadsSlice.ts`).
It is set by the `turn_started` WebSocket event and cleared by
`turn_completed` / `turn_interrupted`. The only way to restore it from the
server is `seedActiveTurn`, fed from `turn` in `GET /api/sessions/{id}/sends`
by `features/composer/useSendActivity.ts`, which runs for the open session
only.

So whenever the browser starts from an empty store, every session that is
not open loses its spinner while its turn keeps running, until that session's
next turn event or until the user clicks the card:

- **A page reload** starts the store empty. Reproduced in `make dev`: a Codex
  session mid-turn, reloaded with another session focused, showed no spinner.
- **A WebSocket reconnect**: `data/useSessionEvents.ts` calls
  `invalidateAll(queryClient)` and then `resetTurnEphemera()`, which clears
  every session's running flags; only the open session re-seeds.

The running-subagent indicator (`runningSubagents`, seeded from
`running_subagents` in the same `/sends` envelope) very likely has the same
gap. Confirm it while working, and fix it the same way if so.

### The change

1. **Carry each live session's turn on the session list.** Add `turn` to
   `WireSessionListItem`
   (`backend/crates/gateway/delta-wire/src/rest/session_list_item.rs`), with
   the same shape and the same resolution as the `turn` of
   `GET /api/sessions/{id}/sends` (`rest/sends_response.rs`, `WireTurn`): the
   session actor's turn state from `SessionInput::QueryLiveState`, and the
   thread from `SessionStore::in_progress_turn_thread`, so the list, `/sends`
   and the turn events name the same thread. A session with no live actor
   reports the idle turn. Add `running_subagents` the same way if step 3
   confirms the subagent gap. The turn state lives in the session actor's
   memory, so no schema change is involved. Query live state only for the
   live sessions (the first page already fetches them by id in
   `interactor/listing/list_sessions_page.rs`), not per closed row. Reuse the
   live-state query `/sends` uses rather than adding a second path to the
   same facts. Regenerate the TypeScript bindings
   (`frontend/packages/gateway/wire-gen/src/generated/`) and update the
   api-mocks session list (`frontend/packages/testing/api-mocks/src/`) to
   report the turn its store holds, and every fixture that builds a
   `SessionListItem`.
2. **Re-seed the running flags from the list.** Whenever a fresh session list
   lands, call `seedActiveTurn` for each listed session, so a reload (the
   first fetch) and a reconnect (the `invalidateAll` refetch) both restore
   the spinners. Keep the guard `useSendActivity.ts` uses against a stale
   read: an authoritative reconcile only from a settled fetch, and
   `dataUpdatedAt` as a trigger so an unchanged payload still re-seeds after
   the reset. Without the guard, a list fetched before a `turn_completed`
   and landing after it brings back a spinner that never goes out. Say in a
   comment how the list seed and the `/sends` seed coexist for the open
   session.
3. **Running subagents.** Check whether a session that is not open loses its
   running-subagent indicator on reload. If it does, seed `runningSubagents`
   from the list in the same place; if not, say why in the report.
4. Update the doc comments that say the seed comes from `/sends` for the
   open session only (`runningThreadsSlice.ts`, `turnLifecycleSlice.ts`
   `resetTurnEphemera`, `useSessionEvents.ts`, `useSendActivity.ts`), and
   `docs/guides/api/` where the session list or the reconnect healing is
   described.

### Out of scope

- How turns start or end on the server.
- Seeding notices (permission, question) for sessions that are not open.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `list_sessions_page_reports_the_turn_in_flight` (delta-usecase listing
      tests): a live session with a turn in flight is listed with an
      `in_flight` turn naming the thread `in_progress_turn_thread` resolves,
      and an idle or closed session is listed with an idle turn.
- [x] A wire test pins the serialized `turn` on a session list item.
- [x] A frontend test shows a session list whose row reports `in_flight`
      lighting `session-running` on a session that is not focused, with an
      empty store (the reload case).
- [x] A frontend test shows that a list fetched before a `turn_completed`
      and settling after it does not bring the spinner back.
- [x] The e2e-fake reconnect suite (`e2e-fake/ws-reconnect.spec.ts`) covers a
      session that is not focused keeping its spinner across a reconnect.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] In `make dev`, a session that is not focused and is mid-turn keeps its
      running spinner across a page reload, without clicking it, and the
      spinner goes out when the turn ends.
