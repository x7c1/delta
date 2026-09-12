---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0912-0345-feat-remove-a-closed-session-from-the-list
created_at: 2026-09-12T03:45:00Z
updated_at: 2026-09-12T06:09:02Z
---

# feat(sessions): remove a closed session from the list

## Overview

A session that has been closed stays in the navigator forever. Closing is
deliberately non-destructive — the pane is torn down but the row, its
transcript and its worktree survive so the session can be resumed — and there
is no way to say "I am done with this one". The list only grows, and the
sessions that matter sink under ones finished weeks ago.

Delta already has the machinery to drop a session's own data: the
`delete_session` port
(`backend/crates/domain/delta-usecase/src/ports/session_store.rs:118-122`,
sqlite impl `store/sessions.rs:238-247`) deletes the `session` row and lets
`ON DELETE CASCADE` clear every child table (threads, messages, sends,
permission requests, subagents, the sync cursor). Today only the
launch-failure path uses it, to reap a `spawning` row whose launch never bound
(`interactor/lifecycle/cancel_launch.rs`, `clean_up_failed_spawn_row`).

Expose it to the user as `Remove` on the session card's kebab menu, offered
only when the session is closed, and only ever removing Delta's own rows.

### Design

1. **What is removed, and what is not.** Only Delta's database rows. The git
   worktree stays on disk, and so do the agent's own files (Claude Code /
   Codex transcripts and state). Close already keeps the worktree so a
   session can be resumed; removal is the user tidying Delta's list, not a
   command to destroy work that may hold uncommitted changes. Say this in the
   usecase's doc comment, because "delete" invites the opposite assumption.
2. **The endpoint.** `DELETE /api/sessions/{id}`, declared in
   `backend/crates/gateway/delta-wire/src/endpoint/table.rs` next to
   `CloseSession`, bound in `backend/crates/apps/delta-server/src/app/mod.rs`
   (`.bind(endpoint::DeleteSession, api::delete_session)`), handled in
   `api/mod.rs` beside `close_session`. No request body; `204 No Content` on
   success, following `delete_clone_root` (`api/mod.rs:382-390`).
3. **Refusals.** Removal is allowed only for a session that is neither open
   nor still starting:
   - unknown id → `Error::SessionNotFound` → **404**, as `close_session`
     already does;
   - still starting (`SessionStatus::Spawning`, or a runtime `launching` /
     `pending` spawn) → the existing `Error::SessionSpawning` → **409** with
     the existing `session_spawning` code;
   - open (a live pane in the registry) → **409** with a new stable code.
     Add one error variant for it rather than reusing `SessionSpawning`: the
     two states are different and the client message differs. Follow the
     precedent in `api/api_error.rs` — a 409 because the target's *state*
     forbids the operation, not because the id is wrong — and give the code
     the same shape as its neighbours (`session_spawning`,
     `launch_option_builtin`, `send_not_releasable`).
   Open-ness is runtime state, not a column (`session_listing.rs:13-27`), so
   the usecase asks the registry the same way the listing does; it must not
   infer it from `SessionStatus`.
4. **Order of operations.** Check state, then delete, then broadcast. A
   refusal must leave the row untouched.
5. **The event.** Add `SessionEvent::SessionRemoved { session_id }`
   (`ports/session_event.rs`) and its wire twin in
   `delta-wire/src/session_event.rs` with the `From` conversion, broadcast by
   the handler the way `close_session` broadcasts `SessionClosed`. A separate
   event, not a reused `SessionClosed`: every open tab has to drop the row,
   which is not what closing means. `make gen` refreshes the TypeScript.
6. **Applying it in the browser** (`data/applySessionEvent.ts`). Model the
   case on the existing `spawn_failed` branch (lines 246-298), which already
   drops a session and hands focus back: call `removeSessionSends` and
   `invalidateSessions`, and when the removed session is the focused one,
   move focus on. **Where focus goes**: the first session in the list cache
   other than the removed one, falling back to the new-session screen
   (`NEW_SESSION_FOCUS`) when there is none. Read that from the cache
   *before* invalidating, so the choice is synchronous and deterministic
   rather than racing a refetch. Use `reconcileFocusedSession` (the
   programmatic setter), not `setFocusedSession`.
7. **The menu item** (`features/navigator/SessionNode.tsx:470-524`). Add
   `{ label: 'Remove', onSelect: …, tone: 'danger' }`, spread in only when
   `!item.open && !spawning` — the exact inverse of the condition that shows
   `Close`, so the two are never offered together and a card always offers
   exactly one of them. Place `Remove` last. No confirmation dialog: opening
   the kebab and picking a red item is already two steps, and Close — the
   other destructive-looking action — has none either. Wire it through a new
   `useDeleteSessionMutation` next to `useCloseSessionMutation`
   (`gateway/api-client/src/query-hooks.ts:414-427`) over a
   `deleteSession` client method (`http.ts`, `requestNoContent` with
   `method: 'DELETE'`, like `deleteLaunchOption`), invalidating the sessions
   query on success. The broadcast event covers other tabs; the mutation's
   own invalidation covers the acting tab without waiting for the socket.
8. **API docs.** `docs/guides/api/sessions.md` gains a
   `### \`DELETE /api/sessions/{id}\`` section with its status list (204 /
   404 / 409 / 500) — `tests/api_docs_cover_every_route.rs` fails otherwise.

### Session-state coverage

Per the operation × state matrix, `Remove` fired against a session in each
state it can be in:

- **closed** — removes the row and its children; 204.
- **open + idle** — 409, row untouched.
- **open + mid-turn** — 409, row untouched (same check; the turn is not
  interrupted).
- **resuming** — a resume window means the session is open, so 409.
- **spawning (launching)** — 409 via the existing spawning error.
- **spawning (pending)** — 409, likewise.
- **unknown id** — 404.

In the UI the item is offered only in the closed state, so the 409s are the
defence against a stale card (another tab reopened the session) rather than
something a user meets in normal use.

### Pipeline notes

- Touches wire types, so run `make gen`; `make gen-check` inside `make check`
  fails on a stale binding.
- `make check` takes over ten minutes; the check phase is expected to run it
  through the driver's long-running path rather than a capped worker call.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `DELETE /api/sessions/{id}` on a closed session returns 204 and deletes
      the session row and its children — threads, messages, sends, permission
      requests, the sync cursor (cargo test: a router test for the status,
      and a store-level assertion in the style of
      `delete_session_cascades_to_children`).
- [x] The same request returns 409 with a stable `code` for an open session
      (idle and mid-turn) and for a session that is still starting — both the
      `launching` and the `pending` sub-state — and the session row still
      exists afterwards in each case (cargo test, router tests asserting
      status and the JSON `code`).
- [x] The request returns 404 for an unknown session id (cargo test, router
      test).
- [x] Removal deletes no files: the usecase touches neither the worktree nor
      the agent's own data — the test fakes record no filesystem or tmux call
      for a successful removal (cargo test, usecase test).
- [x] `DELETE /api/sessions/{id}` is documented in `docs/guides/api/` with
      its status list (cargo test, `api_docs_cover_every_route`).
- [x] The removal event is defined on the wire and its generated TypeScript
      binding is fresh (cargo test for the `From` conversion, plus
      `make gen-check` inside `make check`).
- [x] The session card's menu offers `Remove` when the session is closed and
      `Close` otherwise — never both, never neither (vitest,
      `SessionNode.test.tsx`, covering closed / open / spawning).
- [x] Picking `Remove` issues `DELETE /api/sessions/{id}` for that session
      (vitest with MSW, in the style of the existing close-request test).
- [x] Applying the removal event drops the session from the list and, when
      the removed session was the focused one, moves focus to the first other
      session in the list — or to the new-session screen when there is none —
      while leaving focus alone when a different session was focused (vitest,
      `applySessionEvent` tests).

## Out of scope

- Removing an open or still-starting session, and any "close then remove" in
  one action. The user closes first.
- Deleting the git worktree, its branch, or the agent's own transcript and
  state files.
- Bulk removal, an undo, or an archive state.
- A confirmation dialog.
