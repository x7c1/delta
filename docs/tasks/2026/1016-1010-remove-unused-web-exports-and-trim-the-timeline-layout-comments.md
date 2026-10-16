---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'appendMessage|useOpenSessionMutation|MAIN_LANE_LABEL|void range' -- frontend/packages && ! git grep -n 'Exported for tests' -- frontend/packages/apps/web/src/features/transcript && ! git grep -nE 'resetTimelineExpandedForTests\\(sessionId' -- frontend/packages"
assignee: null
branch: task/1016-1010-remove-unused-web-exports-and-trim-the-timeline-layout-comments
created_at: 2026-10-16T10:11:14Z
updated_at: 2026-10-16T13:53:12Z
---

# refactor(web): remove unused api-client exports and timeline aliases and trim comments that restate the layout

## Overview

A static survey of `frontend/packages` found exports and parameters that
nothing in production reads, and comments that restate the code next to them.
Remove the first group and trim the second.

Unused exports and parameters:

- `appendMessage` in `frontend/packages/gateway/api-client/src/cache.ts`
  (line 92) and its re-export in `src/index.ts` (line 90). Its comment says
  the live channel applies incremental transcript growth through it, but the
  `transcript_updated` handler in
  `frontend/packages/apps/web/src/data/applySessionEvent.ts` (around line 200)
  invalidates the thread query and refetches instead. Delete the function, the
  export and any import that only served it; do not wire up an incremental
  path in its place.
- `useOpenSessionMutation` in
  `frontend/packages/gateway/api-client/src/session-queries.ts` (line 246)
  and `src/index.ts` (line 34). Nothing calls it; the UI resumes a closed
  session by sending to it. Delete the hook and its export. Keep the
  `openSession` client method and the HTTP route: the REST contract is not
  what is unused, only this React wrapper.
- `MAIN_LANE_LABEL` in
  `frontend/packages/apps/web/src/features/transcript/timelineLanes.ts`
  (line 18) is a back-compat alias of `MAIN_THREAD_DISPLAY_NAME` that only
  `timelineLanes.test.ts` reads. Delete the alias and make the test import
  `MAIN_THREAD_DISPLAY_NAME` from `@delta/model`.
- The `range` parameter of `buildGlobalXMap` in the same file (around line
  630) is `void range` and never consulted. Remove it from the signature and
  from every caller (`ThreadTimelineOverlay.tsx`, `timelineLanes.test.ts`,
  `timelineLanes.global-x-map.test.ts`). Keep the time-range computation the
  lane builder uses for other purposes.
- `resetTimelineExpandedForTests(sessionId?)` in
  `frontend/packages/apps/web/src/features/transcript/useTimelineExpanded.ts`
  (around lines 110–120) accepts a session id, but every call passes none, and
  the per-session branch has no user in production or tests. Remove the
  parameter and that branch; keep the whole-cache reset the tests rely on.

Comments to trim, in `timelineLanes.ts`:

- The `buildGlobalXMap` doc comment (around lines 580–625) walks through the
  percentile index formula, the array construction, and the concrete pixel
  spacings (6 px, 4 px, 5 px) even though the mark diameter is an argument of
  the function and the values live in constants and tests. Keep why the idle
  gap is capped (a long pause would crush the conversation), that message
  order is preserved, that neighbouring marks never overlap, and that every
  lane shares one x map so cross-lane jumps line up. Drop the restated
  sizes, formula and loop description.
- `computeCumulativeEffectiveDt` and `computeIdleGapCapMs` (around lines
  686 and 719) say "Exported for tests" but are not exported. Delete the
  sentence.

And in `cache.ts`, delete the `appendMessage` comment with the function.

Trim comments only where they duplicate code; a comment that explains a
constraint, a race, or a reason (for example the reconnect reasoning in
`usePendingSends.ts`) is not in scope and must stay.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `appendMessage`, `useOpenSessionMutation` and `MAIN_LANE_LABEL` no
      longer appear under `frontend/packages` (`git grep` gate appended to
      `check_command`).
- [x] `buildGlobalXMap` no longer takes a `range` parameter: no `void range`
      remains, and every caller compiles with the new signature (typecheck in
      `make check`, plus the `git grep` gate).
- [x] No "Exported for tests" sentence remains in `timelineLanes.ts` and
      `resetTimelineExpandedForTests` takes no parameter (`git grep` gates
      appended to `check_command`).
- [x] The timeline tests still assert the same ordering and dense-cluster
      spacing expectations (frontend tests in `make check`).
- [x] The transcript refetch on `transcript_updated` and the resume-by-send
      path behave as before (existing web tests and the fake e2e suite in
      `make check`).

## Out of scope

- The `openSession` REST route and client method.
- Comments outside `timelineLanes.ts` and `cache.ts`.
- Renaming anything in the composer's pending strip; a separate change covers
  it.
