---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && for i in 1 2 3; do make e2e || exit 1; done && for i in 1 2 3; do make e2e-fake || exit 1; done"
assignee: null
branch: task/1013-1510-fix-e2e-make-the-flaky-specs-deterministic
created_at: 2026-10-13T06:10:00Z
updated_at: 2026-10-13T09:53:07Z
---

# fix(e2e): make the flaky Playwright specs deterministic

## Overview

Three Playwright specs fail intermittently. CI runs both suites, and a red
check blocks a merge, so each one can stall any pull request. Re-running until
green is not acceptable: find each cause and fix it, or — only where a cause
cannot be established after the investigation below — quarantine the spec
with a comment that states what was ruled out.

### 1. `frontend/packages/apps/web/e2e/comms-pane.spec.ts` — "the comms log leaks no scrollable overflow past its own scroll box"

The final assertion `expect(await shellOverflow()).toBe(baseline)` fails in
about half of full mock-suite runs (`make e2e`) with `Expected: 45,
Received: 101`; the spec alone passes. It reproduced on a clean `main`
(e9c2889b) 1 run in 2. An earlier fix (#427) assumed the baseline was read
before the transcript settled and made it wait for the "No messages yet"
state; the failure persists with the same numbers, so that diagnosis was
incomplete.

Find what the extra 56 px is: under full-suite load (the whole mock suite, or
`playwright test --repeat-each` with the suite's worker count), on a failure
record which descendant of `workspace-shell` extends its scroll height
(bottom-most `getBoundingClientRect().bottom`) at both measurement points and
put it into the assertion message. Then fix the cause — the measurement
taken before a layout that has not settled, or a real leak from the pane — not
the tolerance. Do not add sleeps and do not widen the assertion so that it
would also accept a real leak.

### 2. `frontend/packages/apps/web/e2e-fake/ws-reconnect.spec.ts` — the spec around line 246 ("a permission raised entirely during the outage appears after reconnect")

Failed once in `make check` because the Vite dev server refused the
connection; a re-run of `make e2e-fake` passed. Determine why the dev server
was unreachable mid-suite (a server restart in a preceding spec, a port
reuse, the dev server being stopped by a fixture) and fix that.

### 3. `frontend/packages/apps/web/e2e-fake/server-restart.spec.ts:38` — the backend did not come back, and 21 later specs failed in a cascade

`connection-indicator` stayed `closed` for 20 s after the restart, then every
later spec failed with `ERR_CONNECTION_REFUSED` (the Vite server on 5198 did
not answer either). Only once, right after an interrupted `make check` whose
Playwright processes were killed, so stale processes may have been involved.
The log is no longer available. Two things to fix:

- The suite must not cascade: one spec that leaves the backend or the dev
  server down must not fail every later spec with a connection error. Make the
  shared fixture verify (and, where it owns the process, restore) the
  backend's and the dev server's health before each spec, so a failure stays
  attributed to the spec that caused it.
- If the restart path itself can fail to bring the server back (port still
  bound by the killed process, socket not released), make the relaunch wait
  for what it needs, and make the failure name the cause (the relaunched
  server's own log is already kept under `test-results/e2e-fake/boot-<N>/`).

Report in the PR what each investigation found, including what was ruled out.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The full mock suite (`make e2e`) and the full fake-backend suite
      (`make e2e-fake`) each pass three consecutive runs after `make check`
      (in `check_command`).
- [x] The e2e-fake fixture checks the backend's and the dev server's health
      before each spec, so a spec that leaves either down fails alone.

### Before merge (verified outside the check command)

- [x] The PR states the cause found for each of the three specs (or, for any
      quarantined one, what was ruled out).
- [x] `comms-pane.spec.ts` passes `--repeat-each 20` with the suite's worker
      count.
