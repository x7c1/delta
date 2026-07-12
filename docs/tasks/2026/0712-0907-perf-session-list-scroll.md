---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check && grep -qE '\\bmemo\\(' frontend/packages/apps/web/src/features/navigator/SessionNode.tsx"
assignee: null
branch: task/0712-0907-perf-session-list-scroll
created_at: 2026-07-12T09:07:00Z
updated_at: 2026-07-12T12:36:00Z
---

# perf(web): reduce per-scroll render work in the session list

## Overview

Scrolling the left-pane session list feels sluggish on WebKit engines
(observed in a WebKitGTK browser; WebKit is also what a future Tauri shell
would use on Linux/macOS). The list is already virtualized with
`@tanstack/react-virtual` in
`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`, so
DOM size is not the problem. The cost is per-scroll render work layered on
top of the virtualization:

- `SessionNode`
  (`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`) is
  not memoized. `NavigatorPane` subscribes to several store slices
  (connection, rate limits, notices, focused session, …) and react-virtual
  commits state on scroll, so every scroll tick and every unrelated store
  update re-renders all visible rows.
- Every mounted row registers `virtualizer.measureElement` (ResizeObserver)
  for dynamic height measurement; combined with the full-row re-renders this
  causes re-measure churn during scrolling.
- Each row is absolutely positioned with `transform: translateY(...)` and
  carries `shadow-md`, so every row is an independent stacking context
  painting its own box-shadow while scrolling — a paint pattern WebKitGTK
  handles noticeably worse than Blink.

Reduce the per-scroll work while keeping the current visuals and behavior:

1. Wrap `SessionNode` in `React.memo` and make its props memo-friendly
   (stable callback identities via `useCallback`, primitive/stable props
   instead of fresh objects) so a scroll commit or an unrelated store update
   no longer re-renders every visible row.
2. Audit `NavigatorPane`'s store subscriptions: move slices consumed only by
   individual rows down into the row (or select narrowly) so pane-level
   re-renders happen only when pane-level data changes.
3. Keep dynamic row measurement correct (focused-card expansion still
   resizes properly) while avoiding redundant re-measures — e.g. do not
   re-attach `measureElement` on every render.
4. If profiling shows the per-row `shadow-md` paint is still a major cost on
   WebKit after 1–3, propose a cheaper visual equivalent (e.g. border +
   subtle background) as a follow-up note in the PR rather than silently
   changing the design.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `SessionNode` is memoized: `grep -E '\bmemo\(' …/SessionNode.tsx`
      matches (appended to `check_command` as a gate).
- [x] `make check` passes (build, typecheck, tests, lint); existing
      navigator tests still pass, with updates only where they asserted on
      render counts or implementation details.

### Manual / on-hardware (verified by a human before merge)

- [ ] With a session list long enough to scroll (10+ sessions), flick-
      scrolling the pane is smooth in both a Chromium browser and a WebKit
      browser.
- [ ] Focusing a session still expands its card (thread tree) correctly and
      rows below it reposition without overlap or gaps.
- [ ] Live updates (new session activity, rate-limit banner, connection
      state) still reflect in the list without a full-pane visual flash.

## Out of scope

- Changing the virtualization library or the pagination scheme (the
  virtual-range-derived pagination stays as is).
- Batching or caching `useSessionThreadsQuery` beyond what falls out of
  memoization (separate data-layer concern).
- Any visual redesign of the session cards (see item 4 — proposal only).
