---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rn 'useSessionThreadsQuery' frontend/packages/apps/web/src/features/navigator/ --include='*.tsx' --exclude='*.test.tsx'"
assignee: null
branch: task/1011-1416-perf-carry-each-sessions-threads-in-the-session-list
created_at: 2026-10-11T05:16:57Z
updated_at: 2026-10-11T05:55:12Z
---

# perf: carry each session's threads in the session list

## Overview

The navigator draws every session row with its sub-thread tree (expanded by
default, focused or not — `SessionNode.tsx`, the component doc), so a row cannot
be drawn without its session's threads. Today the session list does not carry
them: `GET /api/sessions` returns the sessions, and each mounted row then fetches
its own tree with `GET /api/sessions/{id}/threads`
(`useSessionThreadsQuery(client, item.session.id)` in
`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`). The list is
composed on the client one row at a time.

Measured on a dev desktop build with the request log turned on, a launch
against a database of 23 sessions sent 34 requests, 23 of them
`/api/sessions/{id}/threads` — one per session, although only 7–8 rows were on
screen. The rows that mount are the visible ones plus `SESSION_OVERSCAN = 8` on
each side (`NavigatorPane.tsx`), and on the first render the window is computed
from the 64px `ESTIMATED_SESSION_NODE_HEIGHT` before any row is measured, so
more rows mount (and fetch) than end up on screen. Beyond the request count,
each row first draws without its tree and grows when its response lands, which
makes the virtualizer re-measure while the list settles.

The fix is to deliver a row's data with the list, not to tune the overscan or
the size estimate: those two values are rendering parameters, and once the
threads arrive with the page they no longer cost a request.

### What to change

1. **The session list carries each session's threads.** Each item of
   `GET /api/sessions` (`SessionListItem`, built from `SessionListing` in
   `list_sessions_page`, `backend/crates/domain/delta-usecase/src/interactor/listing/list_sessions_page.rs`)
   gains the session's threads, in the same `Thread` wire shape and order that
   `GET /api/sessions/{id}/threads` returns today (thread metadata only — no
   messages). The page is still bounded as it is now (every live session plus up
   to `limit` closed ones; `limit` defaults to 30 and is capped at 100).
   - Load the threads of all the page's sessions with **one store query per
     page** (a batch method on the `SessionStore` port taking the page's session
     ids, implemented in the SQLite store and the fake store), not one query per
     row — the server must not move the N+1 behind the endpoint.
   - `listing_for` also looks up `main_thread_id` once per row
     (`self.store.main_thread_id`). If the batched thread rows identify the main
     thread unambiguously, derive it from them so the page makes no per-row
     store call for it either; if they do not, leave that lookup as it is and say
     why in the PR.
   - Regenerate the TypeScript wire types (`frontend/packages/gateway/wire-gen`)
     the way the repository does it.
2. **The navigator rows stop fetching threads.** A row reads its threads from
   its list item. No component under `features/navigator/` calls
   `useSessionThreadsQuery` any more (the check command greps for it).
3. **One client-side source for a session's threads.** The focused session's
   workspace (`features/workspace/WorkspaceScreen.tsx`) keeps
   `GET /api/sessions/{id}/threads` for the session on screen, because its tree
   must refresh at once on a branch send (`useCreateSendMutation` invalidates
   `sessionThreads(send.session_id)`) and on the focused session's turn events
   (`refreshFocusedThreads` in `apps/web/src/data/applySessionEvent.ts`), which
   must not turn into refetching every loaded page of the list. Make the row and
   the workspace agree on one source rather than holding two copies that can
   disagree — for example, seed the `sessionThreads(id)` cache from each list
   page as it arrives, and have the focused row read that cache, so a branch's
   new thread shows in the row as soon as the targeted refetch lands. Seeding
   must not itself trigger a request, and an unfocused row scrolling back into
   view must not trigger one either. Choose the mechanism; describe it in the PR.
4. **Freshness stays what it is today or better.** Today an unfocused row's tree
   refreshes only when its query refetches (30s `SESSION_THREADS_STALE_TIME`,
   on remount). With the threads in the list, an unfocused row is as fresh as the
   last list fetch, and the list is already invalidated on the lifecycle events
   (`session_registered` / `session_opened` / `session_closed` and others in
   `applySessionEvent.ts`) and after a send. Do not add a list refetch to the
   per-turn events.

Unread state is computed on the client from WebSocket events
(`unreadByThread`); the server keeps returning only thread ids and timestamps,
and the unread logic is unchanged.

### Out of scope

- Changing `SESSION_OVERSCAN` or `ESTIMATED_SESSION_NODE_HEIGHT`.
- Collapsing or summarising the tree of unfocused rows; every row keeps showing
  its full tree.
- Removing `GET /api/sessions/{id}/threads`; the focused workspace still uses it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A server test lists a page whose sessions have a main thread and
      sub-threads, and each item of `GET /api/sessions` carries exactly that
      session's threads, in the order `GET /api/sessions/{id}/threads` returns
      them; a session on a later page carries its own threads too.
- [x] A use-case test (with the fake store) shows that listing a page loads the
      threads through the batch method once per page, not `list_threads` once
      per row.
- [x] A navigator test renders several session rows from a list response whose
      items carry sub-threads and shows each row's sub-thread tree without any
      `GET /api/sessions/{id}/threads` request.
- [x] A test shows that after a branch send's targeted refetch of the focused
      session's threads, the focused row shows the new thread.
- [x] An e2e-fake test loads the app with several sessions that have
      sub-threads and observes at most one `GET /api/sessions/{id}/threads`
      request (the focused session's) during the launch.
- [x] No file under `frontend/packages/apps/web/src/features/navigator/` other
      than tests calls `useSessionThreadsQuery` (grep in the check command).

### Before merge (verified outside the check command)

- [ ] On this Linux machine, with `make dev` running on a copy of a real
      database (not the installed Delta's data directory) and the browser
      opened on it, the navigator shows the sub-thread trees of unfocused rows
      as before, and the browser's network log shows no per-row
      `/api/sessions/{id}/threads` requests at load.
- [ ] In the same setup, a branch send in the focused session makes its new
      thread appear in the row and the workspace.
