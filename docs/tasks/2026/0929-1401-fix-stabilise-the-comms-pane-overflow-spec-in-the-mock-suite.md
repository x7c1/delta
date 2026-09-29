---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0929-1401-fix-stabilise-the-comms-pane-overflow-spec-in-the-mock-suite
created_at: 2026-09-29T14:01:00Z
updated_at: 2026-09-29T16:12:00Z
---

# fix(e2e): stabilise the comms-pane overflow spec in the mock suite

## Overview

`frontend/packages/apps/web/e2e/comms-pane.spec.ts:132` ("the comms log
leaks no scrollable overflow past its own scroll box") fails in roughly half
of full mock-suite runs (`make e2e`, and therefore `make check`), while
passing 10 out of 10 when run alone. It has failed three pre-PR checks in a
row today. The failure is always the final assertion:

```
expect(await shellOverflow()).toBe(baseline);
Expected: 45
Received: 101
```

The spec sets a 1280×300 viewport, focuses the Codex session row, records
the workspace shell's `scrollHeight - clientHeight` as `baseline`, opens the
Comms pane, checks the pane's own scroller overflows, and then asserts the
shell's overflow is unchanged. It is a real regression guard (the
`sr-only` direction spans escaping the pane's clip once handed the shell
thousands of px of overflow), so the assertion must stay; what has to go is
the timing dependence.

### Change

1. **Find what the extra 56 px is.** Reproduce under load (run the whole
   mock suite, or `playwright test --repeat-each` with several workers)
   and, on a failure, record which element extends the shell's scroll
   height: evaluate the bottom-most descendant of `workspace-shell` by
   `getBoundingClientRect().bottom` at both measurement points and log it
   into the assertion message. Decide from that whether the baseline is
   taken too early (content of the focused session still rendering, fonts
   or the virtual list settling, so the "baseline" is smaller than the
   settled state and the delta is not the comms pane's doing) or whether
   the pane really leaks under some ordering. Put the finding in the PR.
2. **Fix the cause, not the window.** If the baseline is measured before
   layout has settled, wait on a condition that means "settled" — the
   focused session's transcript rendered (`expect(...).toBeVisible()` on
   its last row, `document.fonts.ready`, an `expect.poll` that the
   shell's overflow has held the same value across two consecutive
   reads) — and take the baseline only then; take the after-measurement
   the same way. Do not add a sleep, and do not widen the assertion to a
   tolerance that would also swallow a real leak. If the pane does leak
   under some ordering, fix the pane (the spans' anchoring) and keep the
   spec as it is.
3. **Prove it.** Run the spec 20 times in a row in the full-suite
   configuration (`--repeat-each 20` with the suite's worker count, or the
   whole suite three times) and record the result in the PR. Leave the
   spec's intent comment accurate for what it now waits on.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `e2e/comms-pane.spec.ts` "leaks no scrollable overflow" passes under
      the full mock suite's parallel run, with the shell-overflow assertion
      still an exact equality against a settled baseline.
- [x] `make check` is green.

### Manual / on-hardware (verified by a human before merge)

- [ ] The PR names what the extra overflow was and shows the spec passing
      20 consecutive runs in the full-suite configuration.

## Out of scope

- The fake-lane flakes tracked separately (echo-deadline, queued-prompt,
  ws-reconnect, fake-claude `full_loop`).
- Any change to the comms pane's behaviour beyond the anchoring fix, if
  that turns out to be the cause.
