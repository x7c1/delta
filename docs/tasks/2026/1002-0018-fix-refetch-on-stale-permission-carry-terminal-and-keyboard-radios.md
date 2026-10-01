---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1002-0018-fix-refetch-on-stale-permission-carry-terminal-and-keyboard-radios
created_at: 2026-10-01T15:18:48Z
updated_at: 2026-10-01T15:44:19Z
---

# fix(web): refetch a session on a stale permission answer, carry the New session terminal over, and keep the default radio keyboard-operable

## Overview

Three small frontend defects, each in code that already exists and each with a
local fix. They are bundled because they share the same review audience and the
same gate (`make check`, the web unit tests and the mock/fake e2e suites).

### 1. A tab that missed a session's close still says "answer in the terminal"

`PermissionNoticeCard` (`frontend/packages/apps/web/src/features/transcript/PermissionNotice.tsx`)
shows the closed-session copy ("This request can no longer be answered — the
session was closed.", Dismiss only) when the decision endpoint answers
`409 permission_not_pending` and its `sessionClosed` prop is true.
`sessionClosed` comes from `TranscriptPane` as `readOnly && !spawning`, i.e. from
the tab's own idea of whether the session is open. A tab whose live socket went
silently half-open never hears `session_closed`, keeps the session open, and on
the 409 points the user at a terminal that no longer exists.

Fix: in the `permission_not_pending` branch of `decide`, invalidate that session's
query (and the sessions list, if the open/closed state is read from there) so the
next render reflects the server's state; when it turns out closed the card shows
the closed-session copy through the path that already exists. Check whether the
`AskUserQuestion` card's `question_not_pending` branch has the same gap and give it
the same treatment if so.

### 2. On the small layout, a terminal opened on the New session screen closes once the session spawns

Terminal open/closed state is per session (`frontend/packages/apps/web/src/store/navStore.ts`,
`terminalOpenBySession`, read through `isTerminalOpen(state, isLargeScreen)`); the
New session screen keeps its own flag (`terminalOpenWithoutSession`, closed by
default). Unset means open on the large layout and closed on the small one, where
the terminal is an overlay. So on the small layout, opening the terminal overlay on
the New session screen and then sending leaves the spawned session with no entry,
which reads as closed: the overlay the user just opened disappears.

Fix: when a send from the New session screen spawns a session, carry
`terminalOpenWithoutSession` over to that session's entry when it is true. The
large layout is unaffected (its default is already open).

### 3. Arrow keys on the default launch-option radios lose focus after one step

`LaunchOptionGroup` in `frontend/packages/apps/web/src/features/settings/SettingsView.tsx`
binds native radios straight to server state: each change fires a clear-then-set
`PATCH` pair, and while it is in flight (`switching`) every radio in the group is
`disabled`, which drops keyboard focus. A native radio group moves the selection on
every arrow key, so a keyboard user gets one step, loses focus, and has to click
back in; passing over intermediate options also saves each one.

Fix so the group stays keyboard-operable: focus must survive a switch, and arrowing
through the group must not leave the user stranded. Choose the approach (for
example keep the radios enabled and serialize or coalesce the requests, or only
commit on a deliberate action) and explain the choice in the PR. Keep the existing
safeguards the component documents (the dangerous-row rule, the stale-default
handling) intact.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A component test shows that a `409 permission_not_pending` on a session the
      tab believes open triggers a refetch of that session, and that the card shows
      the closed-session copy once the refetched session is closed.
- [x] If the question card has the same gap, an equivalent test covers
      `question_not_pending`; otherwise the PR says why it does not apply.
- [x] A navStore (or WorkspaceScreen) test shows that on the small layout a session
      spawned from the New session screen with the terminal open starts with its
      terminal open, and one spawned with it closed starts closed.
- [x] A SettingsView test shows that moving the default with the arrow keys keeps
      focus inside the radio group across a switch.

### Manual / on-hardware (verified by a human before merge)

- [ ] On a phone-width window, open the terminal on the New session screen, send,
      and the spawned session shows its terminal.
- [ ] In Settings, arrowing through a provider's default launch options with the
      keyboard feels natural and ends with the intended default saved.
