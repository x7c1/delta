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
branch: task/1002-0530-fix-web-keep-a-failed-session-card-in-view
created_at: 2026-10-01T18:50:11Z
updated_at: 2026-10-01T19:12:29Z
---

# fix(web): keep a failed session's card in view, and test the refused release

## Overview

### A failed launch sinks out of sight

The navigator lists sessions open-first and virtualizes the list
(`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`,
`SessionNode.tsx`). A launch that fails turns its card into a failed card, which sorts
with the closed sessions, below every open one. Since Delta now re-adopts the sessions
that survive a restart, open sessions accumulate, and a failed card easily lands below
the rendered part of the list: the user who just launched it sees nothing happen and has
to scroll to find out it failed. The e2e-fake `spawn-failure` spec ran into exactly this
and now scrolls the card into view itself (`revealInNavigator` in
`e2e-fake/support/app.ts`); a user has no such helper.

Make a failed launch visible where the user is looking. Decide the mechanism from how
the navigator and the spawn-failure flow work today (read the `spawn_failed` handling in
`frontend/packages/apps/web/src/data/applySessionEvent.ts` and how a failed card is
rendered and focused): for example keep a session the user just launched, failed or
starting, at the top of the list until they act on it, or scroll the navigator to the
focused session's card when it changes state. Explain the choice in the PR. Keep the
open-first ordering for everything else. Once the user's change is in, simplify the
e2e-fake `spawn-failure` spec so it no longer needs to scroll the card into view itself
(drop `revealInNavigator` if nothing else uses it).

### A test for the refused release

When a held send's release is refused with `409 session_hooks_unreachable`,
`frontend/packages/apps/web/src/features/composer/PendingQueue.tsx` shows the
lost-contact message and invalidates the sessions query so the list brings the Close
menu item and the notice. Nothing tests that. Add a component test in
`PendingQueue.test.tsx` that pins both.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A component or store test shows that a session whose launch fails while other
      sessions are open is shown where the user is looking (per the mechanism chosen),
      and that the open-first ordering of the remaining sessions is unchanged.
- [x] The e2e-fake `spawn-failure` spec finds the failed card without scrolling the
      navigator itself.
- [x] A `PendingQueue` component test shows that a release refused with
      `session_hooks_unreachable` displays the lost-contact message and invalidates the
      sessions query.

### Manual / on-hardware (verified by a human before merge)

- [ ] With many open sessions, a launch that fails is immediately visible in the
      navigator without scrolling.
