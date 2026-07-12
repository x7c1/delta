---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0712-1310-ci-e2e-failure-artifacts
created_at: 2026-07-12T13:10:00Z
updated_at: 2026-07-12T16:47:00Z
---

# ci: upload Playwright failure artifacts from the e2e jobs

## Overview

When a Playwright e2e test fails in CI, the failure is currently
undiagnosable after the fact. A recent mock-mode failure of
`e2e/pending-lifecycle.spec.ts` ("the running indicator clears when a turn
is interrupted" — `getByTestId('session-running')` never became visible
within the 5 s expect timeout) illustrated the gap: Playwright wrote
`test-results/…/error-context.md` (an ARIA snapshot of the page at failure
time) on the runner, but the CI workflow uploads no artifacts from the
frontend job, so the snapshot was discarded with the runner. The failure
did not reproduce locally (50/50 passes), and without the page snapshot
there was no way to tell whether the row was missing, the list was empty,
or the whole app failed to boot — the investigation dead-ended at
"probably CI load".

Make e2e failures in CI self-diagnosing:

1. In `.github/workflows/ci.yml`, add an `actions/upload-artifact` step to
   the jobs that run Playwright (the frontend job's mock-mode e2e step, and
   `e2e-fake` if it does not already upload equivalent evidence) that
   uploads the Playwright output directory (`test-results/`, which contains
   `error-context.md` per failure) with `if: failure()` and a short
   retention (e.g. 7 days).
2. Consider enabling Playwright's `trace: 'on-first-retry'` (and
   `retries: 1` in CI only) in `playwright.config.ts` so a flaky failure
   yields a full trace on the retry — but only if the added CI time is
   negligible; the artifact upload in item 1 is the required minimum.
3. Keep local runs unchanged: no retries, no trace, no new local output
   noise beyond what Playwright already writes.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes.
- [x] The CI workflow YAML parses and the new step(s) are gated with
      `if: failure()` (or `if: ${{ failure() }}`), so green runs upload
      nothing.

### Manual / on-hardware (verified by a human before merge)

- [ ] A deliberately-broken e2e run on the task branch (temporary commit,
      reverted before merge) produces a downloadable artifact containing
      the failing test's `error-context.md`.
- [ ] A green CI run uploads no artifacts and its duration is not
      meaningfully longer than before.

## Out of scope

- Fixing the `pending-lifecycle:56` flake itself (tracked separately as an
  observation; this task provides the evidence needed to fix it if it
  recurs).
- Any change to test code or assertions.
