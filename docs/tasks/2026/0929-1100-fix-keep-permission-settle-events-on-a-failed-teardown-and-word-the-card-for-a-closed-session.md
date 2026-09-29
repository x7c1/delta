---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0929-1100-fix-keep-permission-settle-events-on-a-failed-teardown-and-word-the-card-for-a-closed-session
created_at: 2026-09-29T11:00:58Z
updated_at: 2026-09-29T15:20:00Z
---

# fix(permission): keep the settle events when a teardown step fails, and tell a closed session's card it can no longer be answered

## Overview

Two loose ends of delta#415 (settle pending permission requests when a bound
session is torn down), both found while reviewing it and both about what the
browser is told when a session closes with a permission dialog open.

### 1. A failing teardown step drops the settle's events

`Interactor::tear_down_bound_session`
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/tear_down_bound_session.rs:142-148`)
settles the pending permission requests first — denying their rows in the
store and building the `PermissionResolved` events that clear the browser's
cards — and then runs `apply_turn_input(TurnInput::Close)` and
`sweep_running_subagents_on_process_gone`, each with `?`. If either fails
(a store error), the function returns early and the events already built
are dropped: the rows are denied, so a retry produces nothing, and the card
stays on screen with buttons that will only ever get a 409.

By that point the teardown is past the point of no return — the transcript
was synced, the binding removed (`remove_open`), the pane killed (or found
gone), the adapter closed — so there is nothing a caller can undo, and
stopping half-way only loses notifications. Change the routine so a failure
in those last two steps is logged at `warn` (with the session id and which
step failed) and the routine carries on, returning the events it has. Keep
the same shape for both callers (`close_session.rs:153`,
`close_if_pane_vanished.rs:110`). If a step's error carries something the
caller must know (check what `apply_turn_input` and the sweep can actually
fail with), prefer returning the events together with the error over
silencing it — but do not let a returned error discard the events either
way. Update the doc comment on the routine, which currently describes the
settle-before-close ordering, to say why the tail is not fail-fast.

Tests (usecase, fake store/driver in `interactor/testing/`): a teardown
whose `TurnInput::Close` fails still yields the `PermissionResolved` for
the pending request and the session ends up closed; same for a failing
subagent sweep. Follow the structure of the existing teardown tests from
delta#415 rather than adding a new fixture style.

### 2. The permission card points at a terminal that is gone

`PermissionNoticeCard`
(`frontend/packages/apps/web/src/features/transcript/PermissionNotice.tsx:385-415`)
handles `409 permission_not_pending` by switching to guidance. For a
terminal provider that guidance is "Answer the prompt in the terminal" with
an Open terminal button, written for the case the 409 was designed around:
the hook's browser-decision wait timed out and the interactive TUI prompt
owns the question now. After a session close the decision gets the same 409
(the row was denied by the settle above), and the card sends the user to a
terminal whose pane no longer exists. Normally the `PermissionResolved`
that arrives with the close removes the card before anyone clicks, so the
stale guidance is only seen on a tab whose live channel was down — which
part 1 makes rarer, not impossible.

The card should know whether its session is closed and, if so, show the
"can no longer be answered" branch with Dismiss only, saying the session was
closed. `TranscriptPane` already has `readOnly` (the focused session is
closed) and `spawning` (`TranscriptPane.tsx:139-149`); pass the closed
state down as a prop next to `providerHasTerminal` rather than having the
card read the store itself, and word the copy so it is distinct from the
existing "already resolved, or the agent connection was lost" text (a
closed session is neither). Keep the terminal guidance for an open session:
that is the timed-out-hook case and it is still right.

Tests in `PermissionNotice.test.tsx`: on a 409 with the session closed the
card shows the closed-session guidance and no Open terminal button,
regardless of `providerHasTerminal`; on a 409 with the session open the
terminal guidance is unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A bound-session teardown whose turn-close or subagent sweep fails still
      returns the `PermissionResolved` events for the requests it settled,
      and the failure is logged (usecase tests, one per failing step).
- [x] Both teardown callers (explicit close and pane-gone close) go through
      the same routine and the existing delta#415 teardown tests still pass.
- [x] On `409 permission_not_pending` the permission card shows
      closed-session guidance with only Dismiss when the session is closed,
      and the unchanged terminal guidance when it is open (component tests).
- [x] `make check` is green.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real server: with a permission dialog open in one tab and a
      second tab's live channel cut (offline in devtools), close the session
      from the first tab and bring the second tab back; confirm the card is
      gone once the tab has resynced (the close settled the request, so the
      resync rebuilds the notices from a state with nothing pending), and that
      the first tab's card cleared on the close itself. The closed-session
      wording on a 409 is covered by the component tests; it is only reachable
      from a tab that missed the close events without noticing the gap.

## Out of scope

- Changing when or why the settle denies the rows (delta#415's behaviour).
- The question card (`QuestionCard.tsx`): its cancel path is a separate
  endpoint and its copy was not reported.
