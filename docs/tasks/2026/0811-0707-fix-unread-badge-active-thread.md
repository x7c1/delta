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
branch: task/0811-0707-fix-unread-badge-active-thread
created_at: 2026-08-11T07:07:09Z
updated_at: 2026-08-11T10:20:00Z
---

# fix(navigator): phantom unread badge from bumps landing on the active thread

## Overview

Live-dogfooding bug: a thread's unread badge in the navigator will not stay
cleared. Selecting the thread clears it once, but selecting a *different*
thread makes a count of exactly "1" reappear on the first thread, and it
stays pinned at 1 indefinitely.

Root cause (traced on main @ 83e4bc8): the unread mechanism is a plain
in-memory per-thread counter (`frontend/packages/apps/web/src/store/live/
unreadSlice.ts` — `bumpUnread` / `clearUnread`, nothing persisted, no
watermark), and its three touch points violate the invariant "a thread the
user is currently looking at is, by definition, read":

- `frontend/packages/apps/web/src/data/applySessionEvent.ts` ~line 78:
  `turn_completed` bumps `event.thread_id`, guarded so the focused active
  thread is never bumped — consistent with the invariant.
- `applySessionEvent.ts` ~line 133: `external_input` **deliberately bumps
  `activeThreadId`, the thread currently on screen** (the wire event carries
  no `thread_id` — `backend/crates/gateway/delta-wire/src/session_event.rs`
  ~line 52 — so the frontend attributes it to whatever is active). A unit
  test asserts this behavior as correct
  (`frontend/packages/apps/web/src/data/applySessionEvent.test.ts` ~line 268
  "badges the focused active thread on external_input").
- The badge is display-suppressed while the thread is active
  (`frontend/packages/apps/web/src/features/navigator/ThreadTree.tsx`
  ~line 140: `unread > 0 && !isActive && !running`), and the counter is
  cleared **only on an `activeThreadId` transition**
  (`frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx`
  ~line 305, the sole `clearUnread` call site).

Net effect: an `external_input` that arrives while its thread is on screen
writes a `1` that is invisible at write time (badge suppressed), is never
cleared (no activation transition occurs — the user is already there), and
pops into view the moment the user selects another thread. Re-selecting
clears it; the next `external_input` recreates exactly one. This reproduces
the report verbatim. It surfaces prominently on sessions that receive
harness-injected prompts (orchestrator/task-notification traffic), where a
prompt that fails the `TASK_NOTIFICATION_PREFIX` check or the
`prompt_echoes_send` correlation lands as `external_input` on the focused
thread (`backend/crates/domain/delta-usecase/src/interactor/hooks/
on_user_prompt_submit.rs` ~line 148).

Fix the frontend so the invariant holds (details are yours, invariants are
not):

- **A thread that is on screen never accumulates unread.** Whether you guard
  the `external_input` bump the same way `turn_completed` is guarded, clear
  on the deactivation edge as well, or both (belt-and-braces for the
  known focus-transition races: `WorkspaceScreen.tsx` skips active-thread
  reconciliation while `threadsQuery.isFetching`, and new-session focus maps
  `activeThreadId` to null) — pick and justify in code comments. Leaving a
  thread must never *reveal* a count for events that occurred while it was
  visible.
- **Legitimate unread is preserved.** `turn_completed` on a non-active
  thread still bumps; the badge still renders for inactive threads and stays
  hidden for the active one; `clearUnread` on activation still works; counts
  above 1 still accumulate while away.
- **The external-input notice card is untouched.** `external_input` also
  writes a dismissible transcript notice
  (`applySessionEvent.ts` ~line 134 → `noteExternalInput`); that path is the
  user-visible record of the input and must keep working exactly as today.
- **Frontend-only.** Do not change the wire shape (adding `thread_id` to
  `ExternalInput` is out of scope), the backend emission sites, or the
  `TASK_NOTIFICATION_PREFIX` matching (a separate concern). Backend crates
  should be untouched; the full check command still runs as a regression
  gate.
- The asserting test at `applySessionEvent.test.ts` ~line 268 encodes the
  buggy semantics and must be rewritten to the new invariant, not deleted.

Operation × state coverage (unread count vs thread state):

- `external_input` arrives while its thread is active and focused → no badge
  appears after switching away (the fixed case).
- `external_input` arrives while the session is focused but `activeThreadId`
  is null (new-session screen / focus-transition window) → no phantom count
  on whichever thread becomes active next.
- `turn_completed` on a non-active thread → badge appears with a count,
  increments on further turns, clears on selecting the thread (unchanged).
- `turn_completed` for the focused active thread → no badge (unchanged).
- Thread with unread > 0 is selected → count clears; switching away without
  new events → stays cleared (the regression the user hit).
- A subagent-running thread → badge remains suppressed by `running`
  (unchanged).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A unit test at the `applySessionEvent` level proves an
      `external_input` for the focused active thread does not leave a
      residual count that a later thread switch reveals (the old assertion
      at ~line 268 is rewritten to the new semantics).
- [x] A component/workspace-level test covers the full reproduction
      sequence: bump lands while thread A is active → user activates thread
      B → thread A shows no badge; and the legitimate case: turn completes
      on thread A *after* B is active → thread A shows a badge.
- [x] Existing unread tests still pass with unchanged semantics where the
      behavior was correct: badge hidden for the active thread and while
      `running` (`ThreadTree.test.tsx`), session-row dot aggregation
      (`SessionNode.test.tsx`), clear-on-activation and preservation of
      other threads' counts (`WorkspaceScreen.test.tsx`).
- [x] The external-input transcript notice still fires on `external_input`
      (existing notice tests pass unchanged).
- [x] No backend or wire change: `git diff` touches only
      `frontend/` (and this task file); generated wire types are byte-stable
      (hash-compare gate in `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a live session that receives orchestrator/task-notification
      traffic: focus the busy thread, let notifications arrive, switch to
      another thread — no phantom "1" appears on the busy thread; its badge
      appears only for turns that complete while it is genuinely
      out of focus, and selecting it clears the count for good.
