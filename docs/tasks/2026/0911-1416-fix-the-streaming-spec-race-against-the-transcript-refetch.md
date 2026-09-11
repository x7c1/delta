---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -rq "expect(messages).toHaveCount(1)" frontend/packages/apps/web/e2e-fake/ && scripts/e2e-fake.sh --repeat-each=5 e2e-fake/streaming.spec.ts'
assignee: null
branch: task/0911-1416-fix-the-streaming-spec-race-against-the-transcript-refetch
created_at: 2026-09-11T05:16:41Z
updated_at: 2026-09-11T07:16:38Z
---

# fix(e2e-fake): stop the streaming spec from racing the transcript refetch

## Overview

`frontend/packages/apps/web/e2e-fake/streaming.spec.ts` failed in CI on
2026-09-11 (run 34564560109, job 103153906299) with

```
  Error: expect(locator).toHaveCount(expected) failed
  Locator:  getByTestId('message-item')
  Expected: 1
  Received: 3
  Call log:
    10 × locator resolved to 0 elements
    24 × locator resolved to 3 elements
```

at line 47, `await expect(messages).toHaveCount(1);`. Forty other specs in the
same run passed, and the run before it on the same branch — byte-identical
runtime code — passed the whole suite. The failure is a race in the spec, not a
product regression.

### Why it races

The `streaming` scenario
(`frontend/packages/apps/web/e2e-fake/scenarios/streaming.json`) runs
`await_prompt` (writes the user transcript line), `stream_text` (fires the
`MessageDisplay` chunks that render the provisional bubble; writes nothing to
the transcript), `delay { ms: 3000 }`, `reply` (persists the assistant text
line), `tool_use` (persists a second assistant line), and so on.

The spec asserts inside that 3000 ms hold that exactly one `message-item` is
persisted — the user turn — because the streamed reply is still only the
provisional bubble. That assertion holds only if the browser has already
fetched and rendered the user transcript line by the time the hold expires.

The call log shows it had not: the locator resolved to zero elements for the
first ten polls and then to three at once. `toHaveCount` keeps polling until it
matches, so while it waited for the user line to appear the scenario walked
past `reply` and `tool_use`, and the first render that landed carried all three
lines. The value the assertion wanted never existed on screen — it went from
zero straight to three.

Under a loaded CI runner the transcript refetch can simply land later than the
hold, and nothing in the spec or the scenario forces the ordering the assertion
depends on.

### What to build

Make the spec stop depending on catching that transient, without weakening what
it guards. The regression it exists for is the handoff duplicate: the reply
text must never appear twice across the live bubble and its persisted copy,
including in a turn where a `tool_use` line trails the text line. The
assertions after the hold (the provisional bubble disappears, and the reply
text is present exactly once) carry that guard and must stay.

The brittle part is the *total* count pinned mid-turn. Replace it with an
assertion that states what actually matters at that moment — that the streamed
reply is not yet a persisted transcript line — in a form that does not flip
when the user line renders a moment later than the fake expected. Asserting
that no persisted `message-item` carries the reply text while the provisional
bubble shows it is one such form.

Widening the scenario's pre-persist hold is a reasonable companion change if it
makes the observable window robust rather than merely larger: the window exists
so the mid-turn state can be seen at all. Do not rely on a longer wall-clock
delay as the whole fix — a bigger number keeps the same race, just rarer.

Do not change product code. This is a defect in the test's assumptions, and the
suppression gate it exercises is already correct.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `frontend/packages/apps/web/e2e-fake/streaming.spec.ts` no longer pins
      the total persisted `message-item` count while the turn is in flight:
      `grep -rq "expect(messages).toHaveCount(1)" frontend/packages/apps/web/e2e-fake/`
      finds nothing (this grep is appended to `check_command`).
- [x] The rewritten spec still asserts, while the provisional bubble shows the
      streamed text, that the reply is not yet a persisted transcript line, and
      still asserts after the handoff that the bubble is gone and the reply
      text is present exactly once.
- [x] `make check` passes, and the streaming spec passes five consecutive runs
      (`scripts/e2e-fake.sh --repeat-each=5 e2e-fake/streaming.spec.ts`,
      appended to `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [x] The rewritten spec still fails when the regression it guards is
      reintroduced: temporarily defeat the content-based suppression of the
      provisional bubble (the gate behind `streaming-message` in
      `frontend/packages/apps/web/src/features/transcript/` and
      `frontend/packages/apps/web/src/store/live/streamingSlice.ts`) so the
      bubble survives the handoff, run the spec, confirm it fails on the
      duplicate, then restore the code.

## Out of scope

- Product code. The fix is confined to the spec and, if it helps, the
  `streaming` scenario file.
- The other e2e-fake flakes under investigation (the parallel-run failures in
  `echo-deadline`, `queued-prompt` and `ws-reconnect`). They have different
  signatures and are tracked separately.
- Adding a retry policy to the Playwright config. Retries hide flakes rather
  than fix them.
