---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0714-0558-fix-timeline-playhead-navigator-follow
created_at: 2026-07-14T05:58:10Z
updated_at: 2026-07-14T08:41:00Z
---

# fix(web): keep the timeline playhead following navigator thread selection

## Overview

Selecting a thread from the left-pane session list sometimes leaves the
thread-timeline playhead (the vertical line in
`frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx`)
stranded on the previously selected thread. The failure looks intermittent but
is deterministic once its precondition is set: a guard counter leaks and
latches, and every subsequent navigator selection is silently swallowed until
an unrelated timeline interaction happens to repair the counter.

Root cause 1 (primary) — the cross-lane jump guard counter is permanently
latched when the DOM-ready poll times out. `scheduleScrollAfterRender`
(ThreadTimelineOverlay.tsx:620–706) polls per-rAF for the jump target article;
its timeout branch (:677–679) returns without calling `run()`, so neither the
`onScroll` release (:1471–1479) nor the cancel handle (:1485–1488) ever fires,
and `crossLaneJumpInFlightCountRef` (:1268) stays above zero forever. The
comment at :1259–1264 claims the counter is decremented "once the scroll fires
(or times out / is cancelled)" — the timeout leg is not implemented. While
latched, (a) the external-thread effect bails at :1327–1329 after having
already consumed its change-tracking ref at :1312–1315, so navigator-driven
repositioning is dropped without retry, and (b) every IntersectionObserver
flush returns at :1972–1974, so pane→playhead follow is dead too. Timeouts are
routine, not exotic: timeline marks are built for every user/assistant/meta
message (`timelineLanes.ts`, `messageBelongsOnTimeline` :255–261) while the
transcript pane renders only messages passing `messageRendersNothing`
filtering (TranscriptPane.tsx:310–312), so an axis click resolving to a
renders-nothing uuid (e.g. a paired tool-result carrier — common in tool-heavy
sessions) polls for an article that can never exist and is guaranteed to hit
the 1000 ms timeout and latch.

Root cause 2 (secondary) — the external-thread effect
(ThreadTimelineOverlay.tsx:1301–1395) consumes `lastObservedActiveThreadIdRef`
(:1312–1315) before checking that the new lane has a large message
(:1337–1345). If the lane's timeline messages have not loaded yet at click
time, the reposition is skipped and the re-fire promised by the comment at
:1330–1335 bails at :1312, so the deterministic commit at :1358–1360 never
happens for that selection.

Fix, in two parts (both must be timing-independent state-machine changes, not
timing tweaks — this component must behave identically across engines,
including WebKit):

1. Make every counter increment have exactly one guaranteed decrement: give
   `scheduleScrollAfterRender` a settle path invoked on success, timeout, and
   cancel (e.g. call the scroll callback from the timeout branch, or introduce
   a distinct `onSettled`), keeping the release idempotent as today. Update
   the comment at :1259–1264 to match reality.
2. Let the newest user intent win: when a navigator-driven `activeThreadId`
   change arrives while a cross-lane jump is in flight, cancel the in-flight
   jump (`pendingScrollCancelRef.current?.()`) and proceed instead of bailing
   — unless the in-flight jump's target thread equals the new
   `activeThreadId` (that prop change is the overlay's own jump echoing back;
   keep today's skip). Do not consume the change tracking until a reposition
   is actually committed, so the `largeSortedMessages` dependency genuinely
   retries once the lane's messages load.

Existing suites that pin behavior that must not regress: "external
active-thread change" (ThreadTimelineOverlay.test.tsx:5695), cross-lane jump
IO guard suites (:3928, :4374), cancel-releases-counter (:4214), counter
balance across a chain (:4687), poll timeout (:3507 — currently passes no
scroll callback, which is exactly the untested edge).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit: `scheduleScrollAfterRender` invokes its settle/scroll callback
      exactly once when the DOM-ready poll times out (extend the timeout test
      at ThreadTimelineOverlay.test.tsx:3507).
- [x] Unit: after a cross-lane jump whose target uuid never renders is driven
      past `SCROLL_DOM_READY_TIMEOUT_MS`, an external `activeThreadId` change
      moves the playhead to the new lane (fails before this fix: latch).
- [x] Unit: an external `activeThreadId` change that arrives while the lane's
      timeline messages are still loading repositions the playhead onto the
      lane's latest large turn once the messages land (fails before this fix:
      consumed change ref, no retry).
- [x] Unit: an external `activeThreadId` change that arrives while a
      cross-lane jump to a different thread is in flight cancels that jump
      and wins; the overlay's own jump echoing back as a prop change is still
      skipped.
- [x] e2e (mock mode, runs under `make e2e`): with a session fixture that
      includes paired tool-call/tool-result messages, click the timeline axis
      on another lane's small-dot region, wait past the DOM-ready timeout,
      then select a different thread from the session list — the playhead
      moves to the selected thread's lane.

### Manual / on-hardware (verified by a human before merge)

- [ ] In a real dogfooding session with tool-heavy threads: interleave
      timeline axis clicks (including small-dot/cluster regions on other
      lanes) with thread selections from the left-pane session list, and
      confirm the playhead and lane highlight follow every selection.
- [ ] Wheel/arrow timeline scrubbing and cross-lane jump clicks still land
      and animate as before (no regression from the guard rework).

## Out of scope

- Filtering renders-nothing messages out of the timeline marks so every dot
  is guaranteed a transcript article (the broken contract documented at
  timelineLanes.ts:250–253). That is a complementary correctness/UX cleanup
  with visible dot-density changes — a separate task.
- Replacing the counter + time-guard pair with a navigation-generation token
  that IO commits must match (larger refactor; valid future direction).
- The transient wrong-anchor on cross-session selection caused by the overlay
  remounting when `activeThread` momentarily nulls (self-heals via IO;
  lowest-impact finding of the same investigation).
