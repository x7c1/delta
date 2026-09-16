---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -q 'e2e-real-gate.test.sh' Makefile && grep -rq 'deferred by the debounce; last attempt' scripts/tests/"
assignee: null
branch: task/0916-2032-fix-repeat-a-red-canary-verdict-on-the-debounce-deferred-tick
created_at: 2026-09-16T20:32:00Z
updated_at: 2026-09-16T22:02:04Z
---

# fix(canary): repeat a red verdict on the debounce-deferred tick, and run the gate's tests in `make check`

## Overview

`scripts/e2e-real-gate.sh` runs each real-agent canary suite only when the
CLI's version changed *and* 24 hours have passed since that provider's last
attempt. A red attempt is deliberately not retried until the CLI updates
again, so the script repeats the red verdict on every later tick — otherwise
the only trace of it is the `FAILURE:` line from the tick that produced it
and every hourly log after that reads green (see the comment in the
"version unchanged" branch, around line 266).

That repetition exists in only one of the two skip branches. The
"version unchanged" branch logs
`<provider>: that attempt did not pass (<result>) and is not retried until
<provider> updates again` and summarises as
`skipped (version unchanged; last attempt: <result>)`. The "deferred by the
debounce" branch just below it (around line 285: the version *did* change,
but less than 24 hours after the last attempt) logs nothing about the last
result and summarises as `skipped (deferred by the debounce)`. Both CLIs
auto-update several times a day, so a red canary is followed by an update
inside the window more often than not — and on that tick the red silently
disappears from the output (observed 2026-09-09).

### Change

1. In `scripts/e2e-real-gate.sh`, make the debounce branch repeat the last
   result the same way the unchanged branch does: when `last_result` is set
   and is not `success`, log the same "that attempt did not pass …" line
   (with the log path when known) and summarise as
   `skipped (deferred by the debounce; last attempt: <result>)`. A green
   last attempt keeps the plain `skipped (deferred by the debounce)`.
   Extract the shared "unresolved verdict" message into one helper used by
   both branches, so the two cannot drift apart again. The existing tail
   "and is not retried until <provider> updates again" is true only on the
   unchanged branch — on the debounce branch the CLI *has* updated and the
   retry waits for the window — so reword the shared line to be true on
   both (for example "and has not been retried since"), and update the
   harness assertions that pin the old tail to the new wording.
2. In `scripts/tests/e2e-real-gate.test.sh`, add a case after the existing
   "a red canary is not retried" section (around line 227): bump the codex
   version file again (`v2.2.0`) **without** ageing the attempt, tick, and
   assert that the tick exits 0, runs nothing, prints the "that attempt did
   not pass (failure (exit 3))" line for codex, and summarises codex as
   `skipped (deferred by the debounce; last attempt: failure (exit 3))`.
   Also cover the green side: bump the claude version file without ageing
   and assert its summary stays `claude: skipped (deferred by the debounce)`
   with no "did not pass" line for claude. Follow the harness's existing
   `assert_eq` / `assert_contains` style.
3. Wire the harness into `make check`. The roadmap recorded that #384 added
   the harness's assertions to the gate, but nothing in the repository runs
   `scripts/tests/e2e-real-gate.test.sh`: no `Makefile` target, no CI step.
   Add a `make` target for it and call it from `check` (after `gen-check`,
   before the frontend stage is a natural slot). The harness stubs the CLIs
   and the suites, needs no network or quota, and runs in seconds — it was
   confirmed green on `main` at authoring time.

### Session-state coverage

Not applicable: the change is to the canary gate script, not to any
operation Delta exposes.

### Pipeline notes

- Shell only, plus one `Makefile` edit. Keep the script portable to bash
  3.2 / stock macOS as #384 made it (no GNU-only flags).
- Both appended gates were negative-tested at authoring time: the
  `Makefile` does not mention the harness on `main`, and no test asserts on
  `deferred by the debounce; last attempt`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A tick whose provider updated inside the debounce window after a red
      attempt prints that provider's "that attempt did not pass" line and
      summarises it as `skipped (deferred by the debounce; last attempt:
      <result>)` (asserted by the new harness case; the assertion's presence
      is pinned by `grep -rq 'deferred by the debounce; last attempt'
      scripts/tests/`, appended to `check_command`).
- [x] A green provider deferred by the debounce keeps the plain
      `skipped (deferred by the debounce)` summary (harness assertion in the
      same case).
- [x] The "version unchanged" tick and the debounce-deferred tick print the
      same "that attempt did not pass" line through the shared helper, with
      a tail that is true on both branches (harness assertions on both
      ticks pin the identical wording).
- [x] `make check` runs `scripts/tests/e2e-real-gate.test.sh`
      (`grep -q 'e2e-real-gate.test.sh' Makefile`, appended to
      `check_command`, and the harness's own pass/fail decides the stage).

## Out of scope

- Changing the debounce length or the version-change trigger.
- The desktop-notification fallback or the lock handling.
- Any change to what the suites themselves check.
