---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make e2e && ! git grep -n -i 'footer' -- 'frontend/packages/apps/web/src/features/transcript/*imeline*' && ! git grep -n -e 'per device' -e '(L927)' -e 'sit on the LEFT' -- frontend/packages/apps/web/src/features/transcript"
assignee: null
branch: task/1022-0840-keep-the-timeline-playhead-visible-beside-long-lane-labels
created_at: 2026-10-22T08:40:12Z
updated_at: 2026-10-22T09:10:22Z
---

# fix(web): keep the timeline playhead visible beside long lane labels

## Overview

When the window is narrow and a subthread's name is long, the timeline
playhead can disappear. Two defects combine:

1. **The lane label column has no width cap.** The lane grid in
   `frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx`
   uses `gridTemplateColumns: 'max-content 1fr'`, so the label column grows to
   the widest label. The label cell in `TimelineLaneRow.tsx` carries
   `truncate`, but nothing bounds its width, so it never truncates. The label
   is `position: sticky; left: 0; zIndex: 1` with an opaque background, so
   whatever part of the axis sits under it, including the playhead, is hidden.
   With a long label on a narrow card only a sliver of the axis stays visible.
   (Also note `truncate` on a `display: flex` element does not draw an
   ellipsis on its text; the ellipsis must actually render.)
2. **The horizontal catch-up re-centres on the wrong band.**
   `useTimelineScrollCatchUp.ts` already accounts for the sticky label when it
   decides the playhead is out of view (`leftEdgeThreshold` adds
   `labelOffsetPx`), but it scrolls to
   `playheadInContent - scrollEl.clientWidth / 2`, the middle of the whole
   viewport. When the label column is wider than half the viewport, that
   target lies under the label, so the catch-up fires and the playhead stays
   hidden.

In addition, the catch-up runs only after a navigation (`userActedTick`), so
narrowing the window never brings a hidden playhead back.

Fix all three:

- Cap the label column at a fraction of the timeline's visible width (about
  40%; name the fraction as a constant). Measure the axis-scroll container
  (`thread-timeline-axis-column`) with a `ResizeObserver` so the cap follows
  the card width. Labels longer than the cap truncate with an ellipsis; the
  full name stays available through the existing `title` tooltip. Every lane
  label still shares one column width, so the playhead segments of all lanes
  stay aligned.
- Make the catch-up land the playhead in the middle of the visible axis band,
  i.e. the part of the viewport to the right of the sticky label column,
  instead of the middle of the whole viewport.
- When the axis-scroll container's width changes while the timeline is
  expanded and has an active message, run the same visibility check and
  scroll the playhead back into the visible axis band if it is outside it.
  The existing behaviour on a fresh mount (no catch-up before the first
  navigation) is unchanged apart from this resize path.

In the same PR, fix the timeline comments that no longer describe the code.
The timeline has moved from a footer to the top region of the transcript
(`TranscriptTopRegion.tsx`), and its expanded state is stored per session:

- `ThreadTimelineOverlay.tsx`: the component doc comment calls it "the fixed
  footer between the conversation pane and the composer" and says the
  preference "is persisted per device"; `TIMELINE_JUMP_BUTTON_CLASS`'s comment
  says the jump buttons "sit on the LEFT of the expanded header row" (they
  sit on the right); the header comment says the wrapper's `pr-3` sets the
  right inset, but the class is `pr-1`; the `noMessages` comment cites
  `(L927)`, a line reference into a file layout that no longer exists; the
  `highlightedThreadId` comment says "freshly-mounted footer".
- Every other "footer" referring to the timeline in `timelineLanes.ts`,
  `useTimelineExpanded.ts`, `useTimelinePaneScrollFollow.ts` and
  `useTimelineStepNavigation.ts`.
- Any comment touched by the fixes above (for example the "v31 fix" notes in
  `useTimelineScrollCatchUp.ts`) must describe the new behaviour.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test renders the timeline with a lane label far wider than the
      timeline's visible width and asserts the label cell's width is capped
      at the configured fraction of the axis-scroll container's width.
- [x] A test asserts that when the label column is wider than half the
      viewport, the horizontal catch-up scrolls so the playhead lands inside
      the visible axis band (right of the label column), not under the label.
- [x] A test asserts that shrinking the axis-scroll container's width, with
      the playhead outside the visible axis band and no new navigation,
      scrolls the playhead back into the band.
- [x] No comment in the timeline sources calls the timeline a footer, says
      its state is persisted per device, says the jump buttons sit on the
      left, or cites `(L927)` (the `git grep` gates appended to
      `check_command`).

### Before merge (verified outside the check command)

- [x] In the running app, with a subthread whose name is longer than half the
      timeline's width, narrowing the window keeps the playhead visible and
      the long label ends in an ellipsis whose tooltip shows the full name.
- [x] Wheel, ArrowLeft / ArrowRight and click navigation still move the
      playhead and keep it visible on a narrow window with long labels.
