---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0712-1559-perf-transcript-virtualization
created_at: 2026-07-12T15:59:00Z
updated_at: 2026-07-12T18:35:00Z
---

# perf(web): virtualize the transcript message list

## Overview

The transcript pane mounts every message of the open thread — with fully
rendered Markdown — into the DOM at once:
`frontend/packages/apps/web/src/features/transcript/TranscriptPane.tsx`
maps `renderedMessages` directly (the `renderedMessages.map(...)` block around
line 1380). On long threads this makes (1) the initial mount and every
cross-thread jump O(N) in thread length, (2) every forced layout read
(composer autosize, overlay measurements) proportionally expensive because the
whole thread shares one layout tree, and (3) WebKit engines noticeably slower
than Chromium — WebKit's layout path amplifies the cost, and prior fixes that
reduced reflow *frequency* (composer autosize coalescing, highlight
compositor-friendliness, session-list memoization) all hit this same remaining
multiplier: the size of the live DOM.

Virtualize the transcript with `@tanstack/react-virtual` so only the visible
window (plus overscan) of messages is mounted. The codebase already has a
worked precedent to model after:
`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx` uses
`useVirtualizer` with ResizeObserver-backed `measureElement` for
variable-height rows (see its comments around lines 289–310), and
`frontend/packages/apps/web/src/test/setup.ts` already stubs ResizeObserver so
virtualized components are testable under jsdom (tests drive the windowed
range explicitly rather than via layout).

A row should be the existing per-message block (the message article plus its
sub-thread chips, currently keyed by `message.uuid`). `MessageItem` is already
memoized. Message heights vary hugely (one-line meta rows vs. long Markdown
with collapsible tool cards that expand/collapse in place), so rows must be
measured, not estimated-only. Tail extras that render after the message list
(the streaming preview bubble, `SubagentRunningIndicator`, the inline
`QuestionCard`) must stay at the conversation tail.

Several existing mechanisms assume every message is always in the DOM and must
be redesigned together with the virtualization — this is the bulk of the task,
not an afterthought:

- **Timeline jump machinery** (`ThreadTimelineOverlay.tsx`):
  `scrollMessageIntoView` (line ~468) resolves the target via
  `container.querySelector` + `scrollIntoView({ block: 'start' })`, and
  `scheduleScrollAfterRender` (line ~620) polls rAF until the target element
  appears. Under virtualization an off-screen message is *never* mounted, so
  the jump must instead resolve the target uuid to a row index and go through
  the virtualizer's `scrollToIndex`, then apply the jump highlight once the
  row actually mounts. The landing offset must still respect the pinned
  top-region height (today via `scroll-margin-top` driven by
  `--delta-top-region-reserve`; `scrollToIndex` does not read CSS
  scroll-margins, so the offset needs explicit handling). Cross-lane jumps
  (thread switch, then scroll after the new thread renders) must keep working,
  including the in-flight-jump guard counter that suppresses the pane→playhead
  follower until the jump settles.
- **Pane→playhead follower** (`ThreadTimelineOverlay.tsx`, effect at
  line ~1938): an IntersectionObserver over `article[data-message-uuid]`
  (`ALL_ARTICLES_SELECTOR`) plus a MutationObserver that observes newly added
  articles. Observing only mounted articles is semantically fine (only visible
  articles can be "topmost"), but the bookkeeping must survive constant
  mount/unmount churn: entries in the `intersecting` map and the `observed`
  set must not go stale when the virtualizer unmounts a row (an element
  removed from the DOM does not reliably emit a leave entry), or the playhead
  will be driven by rows that no longer exist.
- **Stick-to-bottom** (`TranscriptPane.tsx`): several effects pin
  `scrollTop = scrollHeight` (thread switch, content growth, body resize,
  bottom-overlay growth). Under virtualization `scrollHeight` is the
  virtualizer's estimated total size and changes as rows get measured, so
  following the tail (especially during streaming, where the live bubble sits
  after the virtualized rows) must keep the pane pinned without fighting
  measurement-driven scroll-height changes, and must still never yank the
  user while they read scrollback.
- **Breadcrumb "go up" landing** (`TranscriptPane.tsx`, effect at line ~584):
  scrolls a `[data-child-thread-id]` chip into view and flashes it. The chip
  lives inside a message row that may be far outside the mounted window; the
  landing must resolve which message owns the chip and scroll to that row
  first.
- **Branch-origin quote highlight** (`TranscriptPane.tsx`, effect at
  line ~641): paints ranges over `[data-testid="message-item"]` articles.
  Only mounted articles can carry ranges; the highlight must re-apply as rows
  mount/unmount during scroll so visible occurrences stay marked.

The messages query still loads the whole thread (data is not paginated by this
task); only the DOM is windowed.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes (build, typecheck, tests, lint), including the new
      and updated transcript tests.
- [x] With a long thread rendered (e.g. 200+ messages), only the virtualized
      window is mounted: a unit test asserts the number of rendered
      `[data-testid="message-item"]` articles stays far below the total and
      that scrolling the pane changes which messages are mounted.
- [x] A timeline jump to a message outside the mounted window scrolls the pane
      to that message and applies the jump highlight once its row mounts —
      covered by tests for both the same-thread and cross-thread
      (thread-switch, then scroll) paths.
- [x] Stick-to-bottom behavior is covered by tests under virtualization: the
      pane follows appended messages / the growing streaming preview while at
      the bottom, and does not scroll when the user is reading scrollback.
- [x] The pane→playhead follower's bookkeeping cannot be driven by unmounted
      rows: a test exercises window churn (rows unmounting as the pane
      scrolls) and asserts the committed playhead index tracks only
      currently-mounted articles.
- [x] The breadcrumb "go up" navigation still lands on (and flashes) the
      child-thread chip when the chip's owning message starts outside the
      mounted window.

### Manual / on-hardware (verified by a human before merge)

- [ ] On a long thread (100+ messages with heavy Markdown) in a WebKit
      browser: opening the thread and jumping into it from another thread no
      longer pays a whole-thread mount — both feel comparable to Chromium.
- [ ] Typing in the composer on the same long thread reaches effective parity
      with Chromium (the residual per-keystroke gap attributed to full-tree
      layout is gone).
- [ ] With the timeline expanded: scrub-jumps land the target just below the
      pinned top region and flash it; the playhead follows pane scroll; no
      jump/follow ping-pong; repeated jumps to the same message replay the
      flash.
- [ ] Branch-quote and hover-chip highlights still mark visible occurrences
      while scrolling through a long thread.
- [ ] No visible regressions in everyday transcript behavior: scrollback
      reading is never yanked, thread switching lands at the bottom, tool
      cards expand/collapse in place without the list jumping.

## Out of scope

- Paginating or windowing the thread-messages query (data loading is
  unchanged; only the DOM is windowed).
- Virtualizing anything other than the transcript message list (the session
  navigator is already virtualized).
- Changes to message rendering itself (Markdown pipeline, tool-pair
  collapsing, MessageItem visuals).
