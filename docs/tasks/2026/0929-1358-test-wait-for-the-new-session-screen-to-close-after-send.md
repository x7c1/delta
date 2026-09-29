---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rqF \"getByTestId('session-node').first().or(\" frontend/packages/apps/web/e2e-fake/ && scripts/e2e-fake.sh e2e-fake/spawn-failure.spec.ts --repeat-each=5"
assignee: null
branch: task/0929-1358-test-wait-for-the-new-session-screen-to-close-after-send
created_at: 2026-09-29T04:58:24Z
updated_at: 2026-09-29T05:12:14Z
---

# test(e2e-fake): wait for the new-session screen to close after a send

## Overview

`make check` failed once in `e2e-fake/spawn-failure.spec.ts`, test
"a launch that fails while you are elsewhere turns its row failed, and opening
it explains why", 613 ms into its second `startNewSession` call:

```
Error: strict mode violation: getByTestId('session-node').first().or(getByTestId('new-session-empty')) resolved to 2 elements
    at startSessionOn (frontend/packages/apps/web/e2e-fake/support/app.ts:78:5)
```

`startSessionOn` in `frontend/packages/apps/web/e2e-fake/support/app.ts`
begins by waiting for "the app's settled state": either a session node in the
navigator or the cold-start `new-session-empty` placeholder, assuming only one
of the two is ever visible. It ends by clicking Send and returns immediately.
The workspace moves focus from the new-session screen to the new session
after the send, but the helper does not wait for that. The spec then waits
for the spawning row through the REST API only (`sessionWithStatus`) and calls
`startNewSession` again. In that window the new-session screen is still up
while the navigator already lists sessions, so the `.or()` locator matches two
elements. Playwright's strict mode throws at once rather than retrying, so the
test fails whenever the second call lands inside that window. The failure
screenshot shows the settled state a moment later: focus on the spawning
session, no new-session screen.

This is a test-helper defect, not a product bug, and it can hit any spec that
starts a session while others are listed or starts two sessions in a row.
Fix it in the helper so every caller gets it:

1. **Return only once the send has left the new-session screen.** After
   clicking Send, wait until `new-session-empty` is gone (the workspace
   focuses a new session right after the send is accepted). Before adding the
   wait, check every caller of `startNewSession` / `startNewCodexSession`
   under `frontend/packages/apps/web/e2e-fake/` for one that expects to stay
   on the new-session screen after the send (for example a rejected send);
   if one exists, give it a way to opt out rather than weakening the wait for
   everyone, and name it in the report.
2. **Make the entry wait strict-safe.** Wait for the settled state with a
   locator that cannot resolve to two elements, so a legitimate state where
   the navigator lists sessions while the new-session screen is open (for
   example after a reload onto the new-session screen) does not throw. Keep
   the existing decision: click "New session" only when the new-session
   screen is not already showing.
3. Update the helper's doc comment, which currently says the settled state is
   "detected by which signal renders first", to describe what it now waits
   for.

Do not lengthen timeouts to make the race less likely; the wait must be on
the state the next step depends on.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `startSessionOn` no longer uses
      `getByTestId('session-node').first().or(...)`; the grep appended to
      `check_command` fails if that pattern is anywhere under
      `frontend/packages/apps/web/e2e-fake/`.
- [x] `startSessionOn` returns only after `new-session-empty` is no longer
      visible following the send, and the full e2e-fake suite passes under
      `make check`.
- [x] `e2e-fake/spawn-failure.spec.ts` passes five times in a row
      (`scripts/e2e-fake.sh e2e-fake/spawn-failure.spec.ts --repeat-each=5`,
      appended to `check_command`).
