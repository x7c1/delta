---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0714-1339-fix-timeline-jump-pane-tail-stick
created_at: 2026-07-14T13:39:55Z
updated_at: 2026-07-15T08:05:00Z
---

# fix(web): keep the transcript pane on the jump target for timeline-initiated thread switches

## Overview

When the user moves the timeline playhead across lanes (wheel/arrow scrub or
axis click) and the move switches the active thread from A to B, the center
transcript pane switches to B but ends up at B's TAIL instead of the message
the playhead landed on. Reproduced deterministically in mock mode at `main`
(e37f45a, which already includes the two recent playhead fixes); three
mechanisms compound, all rooted in the pane's tail-stick writers firing for a
thread switch they do not own:

All timeline jump paths funnel into the overlay's navigation effect
(`frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx:1507–1605`).
The cross-lane branch (:1546) calls `setActiveThread(B)` (:1572) and
`scheduleScrollAfterRender(container, target)` (:1573). The thread switch then
triggers TranscriptPane's tail writers, all setting `stickRef.current = true`
and `el.scrollTop = el.scrollHeight`: the thread-change stick
(TranscriptPane.tsx:536–549), the content stick (:573–583, re-fed by the
guaranteed `invalidateThreadMessages` refetch from WorkspaceScreen.tsx:269–273
and by streaming chunks), the top/bottom overlay measures (:593–606,
:1025–1058 — the latter keyed on `[bottomContent]`, a fresh JSX identity every
render, so it rewrites the tail on EVERY render while stick is armed).

- **M1 — timeout tail-parking (deterministic):** an axis click on a
  renders-nothing mark (e.g. a `tool_result` carrier; the pane filters them
  via `messageRendersNothing`, toolPairs.ts:63–68) starts a cross-lane jump
  whose target never gets an article. The tail writers park the pane at B's
  tail with `stick=true`; the DOM-ready poll settles at
  `SCROLL_DOM_READY_TIMEOUT_MS` without scrolling (:699–705) — the guard
  counter is correctly released, but nothing corrects the pane. Split-brain:
  playhead on the clicked mark, pane at the tail, indefinitely in a quiet
  session.
- **M2 — bottom-clamp stick re-arm (deterministic for near-tail targets):**
  the wheel velocity staircase (:1750–1757) commonly lands multi-notch bursts
  on B's LAST large turn; `scrollIntoView({block:'start'})` clamps at the
  bottom, the landing's scroll event recomputes `stickRef`
  (TranscriptPane.tsx:506–517) with `distToBottom < STICK_THRESHOLD_PX` and
  re-arms `stick=true`. From then on every content/resize/render write glues
  the pane to the tail, and live content pushes the target off-screen.
- **M3 — stick-armed sub-frame window (racy, hardware-amplified):** `stickRef`
  stays `true` from the switch commit until the corrective scroll's event
  dispatches (~1 frame); any render in that window re-runs the
  `[bottomContent]` effect and yanks to tail. The cross-lane path self-heals
  via its rAF recall; the same-lane branch (:1526–1544, immediate
  `scrollMessageIntoView`, no recall) does not.

Compounding: once the pane tail-follows, the pane→playhead IO follower
(ThreadTimelineOverlay.tsx:2041–2199) commits topmost-visible after the guards
expire and drags the playhead off the user's pick toward the tail region —
both halves end up wrong.

Fix — **navigation-intent handoff** (state-machine invariants, no timers,
following the style of the three prior fixes in this component):

1. The overlay's jump paths record an explicit intent `{targetUuid, threadId}`
   BEFORE switching threads — preferably a single store action (e.g.
   `setActiveThreadWithJumpTarget`) so TranscriptPane's layout effect can read
   it synchronously in the switch commit. TranscriptPane's thread-change stick
   branches on it exactly like the existing breadcrumb precedent
   (`scrollToChildRef`, :540–543): a jump-initiated switch sets
   `stickRef.current = false` and writes NO tail scroll. The intent is
   consumed exactly once via `scheduleScrollAfterRender`'s `onSettled`
   (preserve the existing settle-once contract and counter release).
2. A jump landing never arms stick: the landing path clears
   `stickRef.current` so a bottom-clamped `scrollIntoView` does not re-enter
   follow mode. Only a real user scroll re-arms stick via the existing scroll
   listener (:506–517).
3. Deterministic timeout fallback for a no-article target: when the DOM-ready
   poll settles without scrolling, scroll to the nearest RENDERING neighbor of
   the target (by timeline order within the lane; lane top if none) — never
   leave the pane at the tail.

Unchanged intended behavior: navigator/left-pane selection, branch chip
clicks (TranscriptPane.tsx:1488–1491), and breadcrumb navigation keep today's
jump-to-tail + armed stick (they call plain `setActiveThread` with no intent).
The breadcrumb "go up" chip-scroll path (`scrollToChildRef`) is untouched.

In-scope cleanup (same-PR per repo convention): the bottom-overlay measure
effect keyed on `[bottomContent]` (fresh JSX identity every render) should be
keyed on the real state it derives from, so it stops rewriting `scrollTop` on
every render while stick is armed.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit (TranscriptPane): a thread switch carrying a jump intent does not
      write `scrollTop = scrollHeight` and leaves stick disarmed; a thread
      switch without an intent (navigator/chip path) still jumps to the tail
      and arms stick (existing pinned behavior).
- [x] Unit (overlay/pane integration or overlay): a cross-lane jump landing
      that clamps at the container bottom does not re-arm stick — a
      subsequent content-growth render leaves `scrollTop` unchanged (fails
      before this fix: M2 re-arm glues the pane to the tail).
- [x] Unit: a cross-lane jump to a renders-nothing target settles at the
      DOM-ready timeout with the pane scrolled to the deterministic fallback
      (nearest rendering neighbor / lane top), not the tail; the settle
      callback still fires exactly once and releases the in-flight counter.
- [x] Unit: the bottom-overlay measure effect no longer re-runs on every
      render (keyed on real state, not JSX identity).
- [x] e2e (mock mode, under `make e2e`): an axis click on another lane's
      large dot scrolls the pane to that message (target article visible,
      pane not at the tail) and the playhead stays on it.
- [x] e2e: an axis click on another lane's renders-nothing small mark leaves
      the pane near the target's timeline position (not at the tail) after
      the DOM-ready timeout.
- [x] e2e: the existing `branch-drill-in.spec.ts` bottom-stick assertions
      still pass (navigator/chip-initiated switches keep jumping to tail).

### Manual / on-hardware (verified by a human before merge)

- [x] Wheel/arrow scrub across lanes in a real tool-heavy session: the pane
      shows the message the playhead landed on, and later streamed content
      does not yank the pane (or the playhead) to the tail.
- [x] Axis clicks on other lanes' dots (large and small/cluster regions):
      pane lands on/near the target, playhead stays on the pick, no
      split-brain during live streaming.
- [x] Left-pane thread/child selection, branch chip click, and breadcrumb
      drill-in still open the thread at its tail as before.

## Out of scope

- Replacing the in-flight counter + 200 ms programmatic-scroll window with a
  navigation-generation token carried by IO flushes (long-term direction;
  separate task).
- Filtering renders-nothing messages out of the timeline marks (dot⇄article
  contract; separate task).
- Reworking the pane's stick model beyond the invariants above (e.g. a full
  scroll-ownership state machine across all writers).
