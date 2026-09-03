---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "fn list_sessions_by_ids" backend/crates/domain/delta-usecase/src/ports/session_store.rs && grep -q "open-first" docs/guides/api/sessions.md && [ "$(cd backend && cargo test -q -p delta-usecase -- --list 2>/dev/null | grep -c '"'"': test$'"'"')" -ge 421 ] && [ "$(cd backend && cargo test -q -p delta-sqlite -- --list 2>/dev/null | grep -c '"'"': test$'"'"')" -ge 91 ]'
assignee: null
branch: task/0903-0430-feat-list-live-sessions-before-closed-ones
created_at: 2026-09-03T04:30:00Z
updated_at: 2026-09-03T05:44:05Z
---

# feat(sessions): list live sessions before closed ones

## Overview

The navigator's session list (`GET /api/sessions`) is ordered purely by
recency: `COALESCE(last_activity_at, created_at)` DESC, then `created_at`
DESC, then `id` DESC, pushed into SQL and paged by a keyset cursor
(`backend/crates/gateway/delta-sqlite/src/store/sessions.rs:265-330`,
`backend/crates/domain/delta-usecase/src/session_page.rs`). The `open` flag on
each row is process-runtime state read from the session actor after the page
is fetched (`backend/crates/domain/delta-usecase/src/interactor/listing/list_sessions_page.rs`),
not a stored column.

Day-to-day use has a handful of live sessions among many closed ones, and a
closed session whose transcript was touched recently sits above a live one
that has been idle a little longer. A closed session is never the thing the
user is about to act on, so it has no business leading the list. Change the
ordering to **open-first**: every session that currently has a live pane
comes first, then every closed session, and *within each group* the existing
recency order is unchanged.

"Live" is the actor's `has_live_pane()`
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/open.rs:66`):
a bound session (`open: true`, pane-backed or terminal-less Codex) **or** a
spawn still in flight (`status: spawning`, listed with `open: false` until its
first hook binds it). A spawning session is not closed, and it is the one the
user just started, so it belongs in the leading group from the moment it is
accepted instead of appearing below every open session and jumping up a few
seconds later when it binds. `ensure_session`
(`backend/crates/domain/delta-usecase/src/interactor/routing.rs:205`) already
fans `QueryIsLive` out over `self.sessions.ids()`; reuse that shape.

### Design

Keep the ordering an API guarantee decided by the server, consistent across
pages, and keep the recency query index-backed (the `ORDER BY` must not gain a
liveness key that the expression index `ix_session_recency` cannot serve —
liveness is not a column anyway). Two phases in `list_sessions_page`:

1. **Live set.** Collect the ids whose actor reports `has_live_pane()` (fan
   out `QueryIsLive` over the registry's ids; a session with no actor is not
   live). This is bounded by the number of live panes, so it is small.
2. **First page (`cursor: None`).** Fetch the live sessions' rows through a
   new store port method `list_sessions_by_ids(&[SessionId]) ->
   Vec<SessionPageRow>` that returns them in the list's recency order
   (`COALESCE(last_activity_at, created_at)` DESC, `created_at` DESC, `id`
   DESC) and silently skips ids with no row (an accepted spawn whose row was
   just reaped). Implement it in `SqliteStore` (bind the id list with
   `rusqlite::params_from_iter` / generated placeholders; an empty list returns
   an empty `Vec` without touching the database), in the fake store
   (`backend/crates/domain/delta-usecase/src/interactor/testing/fake_store.rs`),
   and in the `&S` forwarding impl at the bottom of `ports/session_store.rs`.
   Emit these listings first.
3. **Closed stream (every page).** Fetch the existing recency page with
   `limit + live_count` rows, drop the rows whose id is in the live set, and
   keep the first `limit`. Over-fetching by exactly the live count guarantees
   at least `limit` closed rows whenever that many remain, so a page is short
   only when the stream is exhausted. The `next` cursor names the last
   **kept** row (not the last fetched), and is `Some` only when `limit` closed
   rows were kept. The cursor's shape and the store's `list_sessions_page`
   signature are unchanged.

So the first page carries every live session plus up to `limit` closed ones;
later pages carry closed sessions only. Document that in the port doc, in
`SessionPage`/`SessionListing`, and in `docs/guides/api/sessions.md` (the `GET
/api/sessions` paragraph currently says "ordered by most recent activity";
describe the open-first ordering, that `limit` bounds the closed portion and
the first page additionally carries every live session, and use the term
"open-first" — it is a grep gate in `check_command`). Also refresh the
comments that restate the pure-recency order:
`backend/crates/gateway/delta-sqlite/src/migrations/session.rs` header ("The
session list orders by it directly"), the `list_sessions_page` port doc, the
usecase doc comment, `frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx:127`
("Pages arrive most-recently-active first, so concatenation preserves the
global order" — still true, but say what the order is), and the `sessions`
prop doc in `frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx:49`.

A session that closes between two page fetches can appear once more in the
closed stream of a later page (its row now sorts by recency and is no longer
excluded). This is accepted: the browser invalidates the whole list on
`session_opened` / `session_closed` (`frontend/packages/apps/web/src/data/applySessionEvent.ts`),
so the duplicate is refetched away immediately. Name this in the usecase doc
comment rather than adding de-duplication.

### Frontend

No component change is needed: `WorkspaceScreen` flattens the pages in
order and `NavigatorPane` renders them as received. Update the two comments
named above. The list already refetches on `session_opened` /
`session_closed` / `session_registered`, which is what moves a session
between the two groups.

### Mock handler and fixtures

The MSW handler for `GET /api/sessions`
(`frontend/packages/testing/api-mocks/src/handlers.ts:245-275`) mirrors the
backend order; make it mirror the new one: sort live entries (`entry.open ||
entry.session.status === 'spawning'`) before the rest, keep the existing
recency comparator within each group, and page so that the first page holds
every live entry plus `SESSIONS_PAGE_SIZE` others while later pages hold
`SESSIONS_PAGE_SIZE` each. In the seeded data `sess-mock-1` is open and
`sess-mock-2` (closed) has the newer message, so the walk in
`handlers.test.ts` now starts `['sess-mock-1', 'sess-mock-2']` — update that
assertion and its comment, and adjust the first-page length expectation to
the new rule. Add a handler test that a live session with older activity than
every closed one still leads the first page. Update the fixture comment in
`frontend/packages/apps/web/e2e/multi-session.spec.ts:8-10` only if it states
the order.

### Tests

- Usecase (`backend/crates/domain/delta-usecase/src/interactor/listing/tests/`,
  one file per test as the siblings do; see
  `list_sessions_page_marks_a_bound_session_open_and_a_closed_one_not.rs` for
  how to bind and close a session, and
  `list_sessions_page_reproduces_recency_order_across_pages.rs` for how the
  fake store seeds recency):
  - an open session with older activity than a closed one is listed before
    it, and after `close_session` it drops back to its recency position;
  - a live session whose recency would put it beyond the first `limit` rows
    is still on the first page, and walking the cursor chain to `None` lists
    every session exactly once (no gap, no duplicate);
  - a spawning session (accepted, not yet bound) is in the leading group.
- Store (`backend/crates/gateway/delta-sqlite/src/store/tests/sessions.rs`):
  `list_sessions_by_ids` returns the requested rows in recency order with
  their `last_activity_at`, skips unknown ids, and returns nothing for an
  empty list.
- Mock handler tests as described above.

### Session-state coverage

This task changes no operation a user triggers; it changes how the list is
ordered. The listing is exercised in every state the guide names — closed,
open + idle, open + mid-turn (both are `open`, mid-turn does not affect
grouping), resuming (the pane is bound, so live), and spawning (live via the
in-flight spawn) — by the tests above.

### Pipeline notes

- Backend and frontend both change; `make check` covers both (it also runs
  `make gen`, so no wire type changes are expected — `SessionListItem` is
  unchanged).
- The `check_command` gates fail on `main`: `list_sessions_by_ids` does not
  exist, `open-first` is absent from the API doc, and the test counts are
  418 (`delta-usecase`) and 90 (`delta-sqlite`).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `list_sessions_page` lists every session whose actor reports a live pane
      (open, or a spawn in flight) before every closed session, with the
      recency order preserved inside each group; a closed session returns to
      its recency position (usecase tests, `delta-usecase` test count ≥ 421).
- [x] A live session whose recency falls beyond the first `limit` rows is on
      the first page, and walking the cursor chain lists every session exactly
      once (usecase test).
- [x] A new `SessionStore::list_sessions_by_ids` port method returns the
      requested rows in recency order and skips unknown ids (grep gate on
      `ports/session_store.rs`; `delta-sqlite` test count ≥ 91).
- [x] The MSW `GET /api/sessions` handler applies the same open-first rule and
      `handlers.test.ts` asserts `sess-mock-1` leads `sess-mock-2` (vitest).
- [x] `docs/guides/api/sessions.md` describes the open-first ordering and how
      `limit` bounds the closed portion (grep gate for `open-first`).

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, closing the focused session moves its card below
      every open card, and opening a closed session moves it up into the
      leading group, without a reload.

## Out of scope

- A visual separator or heading between the live and closed groups.
- Collapsing, hiding, or archiving closed sessions.
- Changing the recency key or the cursor format.
- Persisting liveness as a column so SQL could sort on it.
