---
status: completed
pipeline_phase: null
plan: null
base_ref: task/1022-0840-keep-the-timeline-playhead-visible-beside-long-lane-labels
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/1022-0947-show-the-timeline-playhead-when-the-timeline-opens
created_at: 2026-10-22T09:47:08Z
updated_at: 2026-10-22T10:25:00Z
---

# fix(web): show the timeline playhead when the timeline opens

## Overview

When the timeline is expanded on a long session, the playhead starts on the
latest message, at the far right of the axis, while the horizontal scroll
stays at 0. On a session whose axis is about 10,000 px wide, the playhead
sits roughly 10,000 px to the right of the visible area. Nothing brings it
into view until the user navigates (wheel, arrow keys, click, or a thread
selection), so opening the timeline shows lanes with no playhead.

`useTimelineScrollCatchUp.ts` deliberately skips this case: the navigation
catch-up is gated on `userActedTick > 0`, and the resize catch-up skips the
first width measurement after mount (0 → w). Remove that gap.

- When the timeline is shown, place the playhead in the visible axis band
  (right of the sticky label column) with the same landing rule the existing
  catch-up uses, through the existing `scrollPlayheadIntoAxisBand` helper.
  "Shown" covers expanding the collapsed timeline and first mounting it
  already expanded (the expanded state is persisted per session).
- Scroll instantly (`behavior: 'auto'`), not smoothly: a timeline that has
  just opened should already show the playhead, not slide the axis across
  the screen.
- The messages behind the lanes are fetched only after the timeline expands,
  so the first width measurement can arrive before there is an active
  message or before the global x map knows its position. Place the playhead
  once both the width and the active message's x are known. Do it once per
  showing of the timeline; after that, only the existing navigation and
  resize paths move the scroll, so a later background refetch never yanks a
  scroll position the user chose with the scrollbar.
- When the playhead already lies inside the band, do not scroll.
- Check whether a timeline that stays expanded while the focused session
  changes remounts the overlay or keeps it mounted. If it stays mounted,
  treat a new focused session as a new showing too, so its playhead is
  placed the same way.

Update the comments that state the old behaviour ("A fresh mount sits at
`userActedTick === 0` and performs no scroll", the resize effect's note that
it skips 0 → w so a fresh mount does not scroll, and any test comment saying
the same) so they describe the new behaviour.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test expands the timeline on a session whose latest message lies far
      outside the visible axis band and asserts that, without any
      navigation, the axis is scrolled instantly (`behavior: 'auto'` or a
      direct `scrollLeft`) so the playhead lands inside the band.
- [x] A test asserts the placement still happens when the lane messages
      arrive after the first width measurement.
- [x] A test asserts no scroll happens on showing when the playhead already
      lies inside the band.
- [x] A test asserts that a later change of the message list (a background
      refetch) after the initial placement does not scroll the axis again
      without a navigation or resize.

### Before merge (verified outside the check command)

- [ ] In the running app, on a long session, expanding the timeline shows the
      playhead inside the visible axis band immediately, without a sliding
      animation, and collapsing and re-expanding does the same.
