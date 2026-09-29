---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -qF 'expect: { timeout: 15_000 }' frontend/packages/apps/web/playwright.real.config.ts && ! grep -rqF \"getByTestId('new-session-empty')).toHaveCount(0\" frontend/packages/apps/web/e2e-real/"
assignee: null
branch: task/0929-1437-test-give-the-real-lane-the-fake-lanes-expect-timeout
created_at: 2026-09-29T05:37:19Z
updated_at: 2026-09-29T05:44:34Z
---

# test(e2e-real): give the real lane the fake lane's expect timeout

## Overview

The real-claude canary smoke (`frontend/packages/apps/web/e2e-real/smoke.spec.ts`)
starts its session through the shared helper `startNewSession` in
`frontend/packages/apps/web/e2e-fake/support/app.ts`. That helper now waits,
right after clicking Send, for the new-session screen to close
(`await expect(newSessionEmpty).toBeHidden()`), with no explicit timeout.

The two lanes run that wait under different defaults:

- `playwright.fake.config.ts` sets `expect: { timeout: 15_000 }`, with a
  comment explaining why: every observable sits at the end of a real
  multi-hop loop against a real backend, and Playwright's 5 s default is
  calibrated for in-browser UI.
- `playwright.real.config.ts` sets no `expect` timeout, so the same wait gets
  the 5 s default, although the real lane drives a slower backend than the
  fake one.

The smoke spec still carries its own wait for the same state right after the
helper returns:

```ts
await expect(page.getByTestId('new-session-empty')).toHaveCount(0, {
  timeout: 60_000,
});
```

So what used to be a 60 s allowance is now cut off at 5 s inside the helper,
and the spec's own wait can never be the one that decides. The screen closes
as soon as the POST is accepted, before the real spawn registers, so 5 s is
expected to be enough — but the real lane is run by hand and rarely, and a
red canary has to be diagnosed against real upstream drift, so it should not
carry a timeout that differs from the fake lane for no reason.

1. Give `playwright.real.config.ts` the same `expect: { timeout: 15_000 }`
   as the fake config, with a short comment that points to the fake config's
   reasoning rather than repeating it.
2. Remove the smoke spec's now-redundant `new-session-empty` wait. Keep the
   `pending-item` assertion before it and update the comment above them so it
   still says what the spec observes after Send (focus switches to the real
   session before the spawn registers; the helper has already waited for
   that).
3. Check whether any other Playwright config or spec outside `e2e-fake/`
   imports `e2e-fake/support/app`; if one does and runs without an
   `expect` timeout, apply the same change there.

Out of scope: the per-assertion timeouts later in the smoke spec (the 60 s
and 30 s waits for the real reply), which are about model latency and stay.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `playwright.real.config.ts` sets `expect: { timeout: 15_000 }` (the
      grep appended to `check_command` requires it).
- [x] No spec under `e2e-real/` waits for `new-session-empty` to reach count
      0 any more (the negated grep appended to `check_command` fails if one
      does), and the frontend typecheck and lint pass under `make check`.

### Manual / on-hardware (verified by a human before merge)

- [ ] The next manual run of `make e2e-real-claude` passes the smoke spec.
      This can follow the merge: the lane consumes Claude quota and is run by
      hand when the CLI version changes.
