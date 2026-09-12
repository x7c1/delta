---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0912-0330-feat-mark-the-subthread-with-the-newest-activity
created_at: 2026-09-12T03:30:00Z
updated_at: 2026-09-12T07:09:23Z
---

# feat(navigator): mark the sub-thread with the newest activity

## Overview

Coming back to a session from somewhere else lands the user on the main
thread, and the navigator gives no clue which sub-thread they were last
talking in. `ThreadTree`
(`frontend/packages/apps/web/src/features/navigator/ThreadTree.tsx`) orders
sub-threads by creation and carries three signals per row — the active row's
accent styling (line 114), a running spinner (lines 126-134), and an unread
badge (lines 140-142). None of them says *recent*, so on a session with a
dozen sub-threads finding "the one from a minute ago" means opening rows until
one looks familiar.

Delta already keeps exactly this information one level up: `session` carries a
denormalized `last_activity_at` column, recomputed as `MAX(message.created_at)`
inside the same transaction that upserts messages
(`backend/crates/gateway/delta-sqlite/src/store/messages.rs:117-133`), read
back as `COALESCE(last_activity_at, created_at)` for the session list. The
`thread` table (`migrations/thread.rs`, the v3 baseline) has no equivalent,
so the frontend cannot rank sub-threads by recency at all.

Give `thread` the same column, maintained on the same path, return it in the
threads listing, and let the navigator put one static mark — a heavier font
weight, no colour change — on the sub-thread whose activity is newest.

### Design

1. **Schema (`backend/crates/gateway/delta-sqlite/src/migrations/thread.rs`).**
   Append one `Step::additive(8, …)` to `STEPS` whose SQL batch holds two
   statements: `ALTER TABLE thread ADD COLUMN last_activity_at TEXT;` and a
   backfill
   `UPDATE thread SET last_activity_at = (SELECT MAX(m.created_at) FROM message m WHERE m.thread_id = thread.id);`.
   A step's `sql` is applied as a batch precisely so a column and its backfill
   land together (`migrations/step.rs`), and `ix_message_thread` backs the
   sub-select. Bump `SCHEMA_VERSION` in `migrations/mod.rs` to 8 — the
   existing `the_registry_agrees_with_the_schema_version` test fails
   otherwise. Extend the module doc comment the way `session.rs:11-19`
   documents its own denormalization: the column is derived state, kept
   because the alternative is a `MAX` per thread on every listing.
   **Backfill rather than leaving existing rows NULL**: the messages are
   already in the table, so the value is derivable, and without it every
   session that predates the migration would show no mark until its next
   message — indistinguishable from "main is the newest", which is the one
   thing the absence of a mark is supposed to mean.
2. **Maintenance (`store/messages.rs`, `upsert_messages`).** The function
   already collects the distinct `session_id`s a batch touched and recomputes
   each session's column inside the transaction. Do the same for the distinct
   `thread_id`s, with the identical `UPDATE … SET last_activity_at = (SELECT MAX(created_at) …)`
   shape and for the identical reason (recompute, not batch-max, so re-ingest
   and timestamp-less lines stay correct). Keep the two recomputations
   adjacent and comment the pair once rather than twice.
3. **Read path.** Add `last_activity_at: Option<String>` to `Thread`
   (`backend/crates/domain/delta-model/src/thread.rs`) and to `WireThread`
   (`backend/crates/gateway/delta-wire/src/thread.rs`, plus its `From`),
   and add the column to `THREAD_COLS`
   (`backend/crates/gateway/delta-sqlite/src/store/threads.rs:25-33`) so both
   `thread()` and `list_threads()` carry it. `list_threads` keeps its
   `ORDER BY id` — recency changes what is *marked*, never the order. Run
   `make gen` to refresh `@delta/wire-gen`; `make gen-check` inside
   `make check` fails on a stale binding.
4. **Which row gets the mark.** Add a pure helper next to `buildThreadTree`
   (`frontend/packages/domain/model/src/thread-tree.ts`), generic over a
   shape carrying `id` and `last_activity_at`, returning the id of the thread
   with the greatest `last_activity_at` — comparing the ISO-8601 strings
   directly, as they are UTC and same-format — or `undefined` when no thread
   has one. Break a tie on the larger id, so the mark is always exactly one
   row. Feed it **every** thread of the session, main included: `ThreadTree`
   renders main's children and never main itself
   (`ThreadTree.tsx:48-53`), so when main is the newest the helper names a
   thread the tree does not draw and no row is marked. That is the intended
   reading — no mark means main is where the last message landed.
5. **Rendering.** In the row's `cn(...)` (`ThreadTree.tsx:112-115`), add
   `font-semibold` when the node's id is the newest. Only the weight: no
   colour change, because the active row already owns `text-accent` and the
   two signals must stay distinguishable when they land on different rows.
   `font-semibold` rather than the active row's own `font-medium`, so the mark
   still reads when the two land on the *same* row — otherwise sending a
   prompt in the sub-thread you are viewing would make the mark vanish,
   leaving a tree indistinguishable from the one that means "main is the
   newest". Pass the newest class after the active one: `cn` resolves a
   font-weight conflict last-wins. The mark is static, so it coexists with the
   running spinner and the unread badge and is never suppressed by them. It
   also carries an `sr-only` label, since weight alone reaches no assistive
   technology.
6. **Staying current without a refetch.** The threads query
   (`useSessionThreadsQuery`, `frontend/packages/gateway/api-client/src/query-hooks.ts:109-120`)
   supplies the initial values; live traffic must move the mark while the
   session is open. Add a per-thread latest-activity record to the live store
   alongside the unread counts
   (`frontend/packages/apps/web/src/store/live/unreadSlice.ts` is the shape to
   mirror — a `Record<ThreadId, string>` in its own slice, with a setter that
   only ever moves a thread's value forward) and update it from
   `frontend/packages/apps/web/src/data/applySessionEvent.ts`, on the same
   events that already carry a `thread_id` and mean "a message landed here".
   Update it for **every** such thread, including the one being viewed —
   unlike `bumpUnread`, which deliberately skips the active thread. Where the
   event carries no timestamp, use the client clock; the value is only ever
   compared against other values from the same source. `ThreadTree` then
   ranks each thread by the live value when one exists and the query value
   otherwise.
7. **Do not touch** the tree's ordering, main's absence from it, the unread
   badge's suppression rules, or the session-level `last_activity_at`.

### Session-state coverage

This adds no operation a user can trigger against a session; it adds a
derived mark to a list that is already rendered in every session state. The
one state-shaped case is a session whose threads have no messages at all
(`last_activity_at` is NULL for every thread) — no row is marked, covered by
an Automated criterion below.

### Pipeline notes

- Touches Rust (migration, store, model, wire) and TypeScript (model helper,
  live store, navigator), so run `make gen` after the wire change and
  `make check` covers the rest.
- `make check` takes over ten minutes; the check phase is expected to run it
  through the driver's long-running path rather than a capped worker call.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The v8 step adds `thread.last_activity_at` and backfills it from
      existing messages: a ladder test in the style of
      `the_v6_step_renames_the_send_hold_marker_and_carries_its_values_over`
      builds a database at the prior version with threads and messages,
      applies the real registry, and asserts each thread's value equals the
      `MAX(created_at)` of its own messages and is NULL for a thread with
      none (cargo test, `delta-sqlite` migrations tests).
- [x] `SCHEMA_VERSION` equals the top of the ladder (cargo test, the existing
      `the_registry_agrees_with_the_schema_version`).
- [x] `upsert_messages` maintains `thread.last_activity_at` per touched
      thread: after upserting messages into two threads of one session, each
      thread reports its own newest `created_at`, a later message moves only
      its own thread's value, and re-ingesting the same batch leaves both
      unchanged (cargo test, `delta-sqlite` store tests).
- [x] A thread whose messages carry no `created_at` keeps
      `last_activity_at` NULL (cargo test, `delta-sqlite` store tests).
- [x] `list_threads` returns the new field and the generated TypeScript
      binding for `Thread` carries it (cargo test for the store, plus
      `make gen-check` inside `make check` proving the binding is fresh).
- [x] The newest-thread helper returns the id with the greatest
      `last_activity_at`, breaks ties on the larger id, and returns
      `undefined` when no thread has a value (vitest,
      `frontend/packages/domain/model`).
- [x] `ThreadTree` puts `font-semibold` on exactly one row — the sub-thread
      with the newest activity — and adds no colour class to it, including
      when that row is also the active one (vitest, `ThreadTree.test.tsx`).
- [x] No row is marked when the main thread is the newest, and none when no
      thread has any activity (vitest, `ThreadTree.test.tsx`).
- [x] The mark coexists with the other row signals: a marked row that is also
      running still shows its spinner, and a marked row that is also active
      keeps the accent styling (vitest, `ThreadTree.test.tsx`).
- [x] Row order is unchanged — sub-threads stay in creation order regardless
      of which one is marked (vitest, `ThreadTree.test.tsx`).
- [x] A live session event for a thread moves the mark to that thread without
      a refetch, including when that thread is the one being viewed (vitest,
      over the live store and `ThreadTree`).

## Out of scope

- Reordering the tree by recency, or showing a timestamp / relative-time
  caption on a row.
- Restoring the last-visited thread on return; the mark says where the last
  message landed, which is a different question.
- Any change to the unread badge, the running spinner, or session-level
  `last_activity_at`.
- Marking anything in the session list itself.
