---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/instant-session-focus
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "ingested nothing is excluded" backend/crates/gateway/delta-sqlite/src/store/sessions.rs && grep -q "session_spawning" backend/crates/apps/delta-server/src/api/api_error.rs && grep -q "SessionSpawning" backend/crates/domain/delta-usecase/src/error.rs && grep -q "session_spawning" docs/guides/api/sends.md && grep -q "spawning" docs/guides/api/sessions.md && ! grep -q "stays out of the list" frontend/packages/testing/api-mocks/src/handlers.ts && test -f frontend/packages/apps/web/e2e-fake/scenarios/slow-start.json && grep -lq "slow-start" frontend/packages/apps/web/e2e-fake/*.spec.ts && ! grep -q "once it appears in the list" frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.test.tsx'
assignee: null
branch: task/0826-1840-feat-focus-spawned-session-on-accept
created_at: 2026-08-26T18:40:00Z
updated_at: 2026-08-26T20:47:02Z
---

# feat(sessions): focus a spawned session the moment its first send is accepted

## Overview

Starting a session from the new-session screen keeps the user parked on that
screen until the launch has fully come up. Today the hand-off works like this:

1. `useSubmitSend.ts` posts `POST /api/sends { new_session: true }`; on the
   `201` it calls `trackSpawn` with the REAL ids the server returned — but the
   workspace stays on the new-session screen.
2. The session row was INSERTed as `spawning` before `claude` launched
   (`lifecycle/spawn_fresh.rs`, `insert_spawning_session`), yet
   `GET /api/sessions` deliberately hides it: `list_sessions_page` in
   `backend/crates/gateway/delta-sqlite/src/store/sessions.rs` (~line 260)
   excludes a message-less `spawning` row ("listing it would surface a row the
   browser cannot open, and the optimistic new-session pending chip would
   mis-bind to it").
3. Only when `claude` reaches its prompt does `SessionStart(startup)` bind the
   spawn, flip the row to `active` and broadcast `session_registered`; the
   browser then refetches the list, and the effect in
   `frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx`
   (~line 200, "Hand a tracked spawn over to normal navigation once it
   registers") focuses the session because its id is now *listed*.

Measured over every `delta-server` log on the dogfooding machine, step 3
alone (launch → first hook) takes 0.8–1.1 s typically and 3.4 s at worst; on
top of that the POST itself blocks for as long as the launch preparation takes
(a worktree spawn runs `git fetch` + `git worktree add` inside the request —
that part moves out of the request in a follow-up task on the same integration
branch). While the POST is in flight `sendInFlight` disables the Send button
app-wide (`Composer.tsx`, `submitDisabled`), so no other session can be
started either.

Both exclusion reasons in (2) are gone: the pending chip has bound by real id
since the spawn registry was keyed on the POST response (`spawnsSlice.ts`), and
"a row the browser cannot open" is exactly the state we now want to *show* —
a session that is starting. This task makes the session exist for the user
from the moment the server accepted it:

### What changes

1. **List `spawning` rows.** In `list_sessions_page` drop the message-less
   `spawning` exclusion (the SQL predicate and the paragraph explaining it);
   the recency ordering is unchanged (a message-less row keys on its
   `created_at`, so a just-accepted session sorts at the top). Update the
   sqlite tests that pin the SQL text / the exclusion
   (`store/tests/sessions.rs`, e.g. `list_sessions_page_uses_the_recency_index`
   and any test that spawns a session and asserts it is absent) — a spawning
   row is now listed with `status: "spawning"` and its `open` flag as the
   registry reports it (`has_live_pane` is `true` while a pending spawn is
   recorded). Document it in `docs/guides/api/sessions.md` (`GET /api/sessions`:
   a session is listed from the moment its first send is accepted, with
   `status: "spawning"` until its first hook registers it; a spawn that fails
   disappears from the list — `spawn_failed`) and adjust the
   `session_registered` wording in `docs/guides/api/live-channels.md` that
   presents registration as the moment a session becomes listable. Mirror the
   change in the mock backend: `frontend/packages/testing/api-mocks/src/handlers.ts`
   (`GET */api/sessions`, ~line 218, currently filters `!entry.spawning`) lists
   spawning rows too; update `handlers.test.ts` (~line 346, "keeps the spawning
   row unlisted") to assert the row IS listed with `status: 'spawning'` and that
   `spawn_failed` removes it.
2. **Reject a send to a spawning session cleanly.** With the row visible a
   user can now reach a spawning session's composer. `enqueue_to_thread`
   (`backend/crates/domain/delta-usecase/src/interactor/enqueue/enqueue_send.rs`,
   ~line 128) goes `ensure_open()` → `open_session()` → `claude --resume <id>`
   when no pane handle is bound — which is the case while the spawn is still
   pending, and would launch a second `claude` against a transcript that does
   not exist yet. Add `SessionRuntime::has_pending_spawn()` next to
   `bind_pending_spawn` in `session_actor/runtime/spawn.rs`, and before the
   `ensure_open` call return a new `Error::SessionSpawning(session_id)`
   (`error.rs`, beside `ResumeUnavailable`) when it is set. Map it in
   `backend/crates/apps/delta-server/src/api/api_error.rs` to `409` with the
   stable code `session_spawning` (same shape as `resume_unavailable`), and
   document the code in `docs/guides/api/sends.md`'s error list. Pin it with a
   usecase test in `interactor/enqueue/tests/` (a fresh spawn that has not
   bound; a send to its main thread is `SessionSpawning` and no `open_session`
   / tmux call happens — the fake tmux driver records calls) and a router test
   for the 409 body. The Codex path is untouched (an adapter-backed session
   binds synchronously inside the spawn, so it is never `spawning` from the
   browser's point of view once the POST returned).
3. **Focus on accept.** Rework the spawn hand-off in `WorkspaceScreen.tsx`:
   - The effect at ~line 200 focuses the newest `spawning` tracked spawn as
     soon as it is tracked — no "present in the refetched list" condition —
     but still only while the user is on the new-session screen
     (`isNewSessionFocus`; they may have navigated elsewhere during the POST).
     It no longer calls `clearSpawn`.
   - `useSubmitSend.ts` invalidates the session list right after `trackSpawn`
     (the helper `applySessionEvent.ts` already uses for `session_registered`),
     so the row arrives on the next GET without waiting for an event.
   - The cold-start / stale-focus reconciliation effect (~line 236, "Resolve
     focus once the session list loads") must not stomp a focus whose id is a
     tracked spawn not yet in the list: skip while
     `spawns.some((s) => s.sessionId === focusedSessionId)`.
   - A tracked spawn is now released by the `session_registered` event, not
     by listing: add a `reduceSessionRegistered` to `spawnsSlice.ts` that drops
     the matching `spawning` entry (wire it where the other reducers are
     registered, see `eventReducer.ts`), and update the module doc that says
     the workspace clears it. `reduceSpawnFailed` is unchanged (it flips the
     entry to `failed` so `usePendingSends` can surface Retry / Dismiss).
   - `applySessionEvent.ts` `spawn_failed`: the row was listed now, so
     invalidate the session list (the comment "a message-less spawning session
     was never listed" is obsolete) and, when the failed id is the focused
     session, move focus back to the new-session screen
     (`useNavStore.getState().reconcileFocusedSession(NEW_SESSION_FOCUS)`) so
     the failed chip — which renders on the new-session surface, see
     `usePendingSends.ts` — is where the user lands, with Retry / Dismiss.
   Update `WorkspaceScreen.test.tsx` (~lines 354–435: "focuses a tracked spawn
   by its real id once it appears in the list", "does not steal focus …",
   "keeps the settings overlay open …") to the new contract — the spawn is
   focused before the list contains it, the reconciliation leaves that focus
   alone, the spawn is dropped by `session_registered` — and add the
   `spawn_failed`-on-focused case. `liveStore.test.ts` covers the new reducer.
4. **The starting session's screen.** While the focused session's
   `session.status === 'spawning'`:
   - the composer is disabled with a placeholder saying the session is
     starting (thread the flag from `WorkspaceScreen` → `TranscriptPane` →
     `Composer`'s `ComposerMode` the same way `readOnly` travels — it is a
     mode property, not a separate element; `submitDisabled` gates on it so
     Cmd/Ctrl+Enter cannot bypass it). The first prompt is visible in the
     pending strip exactly once (it is in the session's open-send list as
     `dispatched`; make sure the tracked local send for the same id does not
     render a duplicate on the thread surface — `usePendingSends.ts` merges
     server and local rows for a thread surface, verify with a test);
   - the navigator card (`features/navigator/SessionNode.tsx`, ~line 315, the
     Open / Closed status badge) shows a `Starting` badge instead, and the
     kebab menu hides `Close` for a spawning row (closing an unbound spawn is
     not a supported operation — see the state matrix below);
   - `Composer.test.tsx` and a `SessionNode` test pin both.
5. **e2e.** Mock suite (`e2e/spawn-failure.spec.ts`, `e2e/multi-session.spec.ts`
   new-session flow): after Send the workspace shows the spawned session (a
   `session-node` for it exists, `new-session-empty` is gone) before any
   `session_registered` is emitted; `spawn_failed` returns the user to the
   new-session screen with the Retry / Dismiss row. Fake suite: add
   `e2e-fake/scenarios/slow-start.json` (`"session_start": { "delay_ms": 2500 }`
   plus the `first-send` steps) and a spec that starts a session on it and
   asserts, before the hook fires, that the workspace is on the new session
   (`Starting` badge on its card, composer disabled with the starting
   placeholder, the first prompt in the pending strip), then that the badge
   becomes `Open` and the scripted reply lands. `e2e-fake/spawn-failure.spec.ts`
   (`never-ready`) gains the same "focused first, then back to new-session
   with Retry / Dismiss" assertions.

### Session-state coverage for the operations this task touches

Operation "send to a session" gains the state **spawning** (row exists, no
bound pane, first send `dispatched`): rejected with `409 session_spawning`;
the composer never offers it. The existing states — closed, open + idle,
open + mid-turn, resuming — are unchanged and their tests keep passing.

Operation "focus a session from the navigator" gains **spawning**: the card is
clickable and shows the starting screen; `Close` is not offered on it.

Out of scope: accepting a send to a spawning session as `queued` (a follow-up:
the launch moves out of the POST next, so there is a window with no pane at
all; queueing needs a flush after bind). Moving the launch preparation off the
POST is the next task on this integration branch.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `GET /api/sessions` lists a message-less `spawning` session with
      `status: "spawning"`; the exclusion paragraph is gone from
      `list_sessions_page` (gate appended to `check_command`) and the sqlite
      tests assert the row is listed, at the top by recency.
- [x] A send to a session whose spawn has not bound is rejected with
      `Error::SessionSpawning` → `409 { code: "session_spawning" }` and starts
      no resume (usecase + router tests; gates appended to `check_command`
      for the error variant, the code, and its entry in `sends.md`).
- [x] The workspace focuses a tracked spawn as soon as it is tracked while the
      user is on the new-session screen, the list reconciliation leaves that
      focus alone until the row arrives, `session_registered` drops the tracked
      spawn, and `spawn_failed` on the focused session returns to the
      new-session screen with the Retry / Dismiss row — pinned by
      `WorkspaceScreen.test.tsx` (the old "once it appears in the list" test
      is gone — gate appended) and `liveStore.test.ts`.
- [x] A focused `spawning` session renders a disabled composer with the
      starting placeholder (button and Cmd/Ctrl+Enter), its first prompt once
      in the pending strip, a `Starting` navigator badge, and no `Close` menu
      item — pinned by `Composer.test.tsx`, `SessionNode` and pending-strip
      tests.
- [x] The mock backend lists spawning rows (`handlers.test.ts` updated; the
      "stays out of the list" comment is gone — gate appended).
- [x] The fake suite has a `slow-start` scenario and a spec that observes the
      session focused with the `Starting` badge before the delayed
      `SessionStart` fires, then `Open` with the reply (gates appended for the
      scenario file and the spec).
- [x] `docs/guides/api/sessions.md` and `live-channels.md` describe a session
      as listed from acceptance (gate appended).
- [x] `make check` is green (backend, generated bindings, frontend, both
      Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [ ] On the dogfooding machine, start a new session with a worktree from a
      remote branch: the workspace switches to the new session as soon as the
      POST returns (the launch preparation still runs inside the POST until the
      follow-up task lands), the card reads `Starting` until `claude` reaches
      its prompt, then `Open`, and the first reply streams in as before.
