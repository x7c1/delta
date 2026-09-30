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
branch: task/1001-0504-fix-keep-a-stale-sends-envelope-from-resurrecting-a-resolved-card
created_at: 2026-09-30T20:04:46Z
updated_at: 2026-09-30T20:23:19Z
---

# fix(web): keep a stale sends envelope from resurrecting a resolved question or permission card

## Overview

A resolved AskUserQuestion card can come back and stay on screen. This made
`e2e-fake/ask-user-question-cancel.spec.ts:22` fail locally: the
`question-card` was still there 15 s after Cancel, although the backend had
done everything. Escape was injected, the `is_error` tool_result and the
follow-up reply were ingested and shown, and the turn went to `Idle`.

The card is not held by React Query. It lives in the zustand notices slice,
which receives it in two ways:

- **Live events.** `permission_resolved` and the turn-end sweep clear it
  (`noticesSlice.ts`).
- **Seeds from the sends envelope.** `usePendingSends.ts` calls
  `seedQuestion` / `seedPermission` from the `GET .../sends` response. Those
  setters (`noticesSlice.ts`) set the notice only when the current one is null
  or has a different request id.

The failing sequence:

1. A `GET .../sends` request started while the question was still pending. It
   overlapped the cancel and was assembled about 1 ms before `Stop`, so its
   body still carries the question.
2. The live `permission_resolved` and `turn_completed` events clear the card.
3. The stale response then resolves, and the seed effect in
   `usePendingSends.ts` runs `seedQuestion(staleQuestion)`. The current
   notice is null, so the setter's same-request-id guard does not stop it, and
   the card reappears.
4. The next sends response carries `question: null`. Seeding null is a no-op,
   so the card is never cleared. The turn-end sweep has already passed.

Two things make this possible:

- `applySessionEvent.ts` treats `permission_resolved` and `question_asked` as
  "pure UI notice" events and does not invalidate the session's sends. The
  `subagent_*` cases right below them do, to guard against exactly this "stale
  envelope overwrites the event" race.
- The question/permission seed effect is not gated on a settled fetch. The
  active-turn seed in the same hook is.

`seedPermission` has the same shape, so the permission card can come back the
same way.

Fix:

- Make a resolved request id stick, so a stale envelope cannot re-seed a card
  that a live event has already resolved. For example, keep the resolved
  request ids per session in the notices slice and have
  `seedQuestion` / `seedPermission` refuse them. A later genuinely new request
  has a different id and still seeds normally.
- Also invalidate the session's sends on `permission_resolved`, as the
  `subagent_*` cases do. A fetch that is in flight is then replaced by one
  that reflects the resolution.
- Gate the question/permission seed on a settled fetch, like the active-turn
  seed.

Apply the same treatment to questions and permissions, and write down the
reasoning in a comment next to the seed setters.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A notices-slice unit test: after `permission_resolved` clears a question
      card, `seedQuestion` with the same request id leaves the card cleared.
      A different request id still seeds.
- [x] The same unit test for `seedPermission`.
- [x] A `usePendingSends` (or `applySessionEvent`) test reproduces the order
      "stale sends response carrying the question resolves after the resolve
      event" and asserts that the card stays cleared.
- [x] `permission_resolved` invalidates the session's sends (unit test on
      `applySessionEvent`).
- [x] `make check` passes, including the fake e2e lane.
