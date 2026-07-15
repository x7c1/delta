---
status: completed
pipeline_phase: null
plan: null
base_ref: task/0714-1339-fix-timeline-jump-pane-tail-stick
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0715-0439-fix-playhead-follower-reserve-line
created_at: 2026-07-15T04:39:26Z
updated_at: 2026-07-15T08:05:00Z
---

# fix(web): keep the timeline playhead on the scrub target when the pane follower flushes late

## Overview

While the user scrubs the timeline playhead rightward with the mouse wheel,
the playhead is sometimes yanked back LEFT one (or more) marks shortly after
they pause. Root-caused deterministically at b2bcf0b (unit-level repro): the
pane→playhead IntersectionObserver follower commits the topmost-visible
article, and after any programmatic `scrollIntoView({block:'start'})` the
topmost-visible article is systematically the message BEFORE the scroll
target — the only thing preventing that wrong commit is a 200 ms time guard,
which routine observer re-binds outlive. This is a pre-existing bug (the
left-direction sibling of the cross-lane counter guard added earlier); it is
independent of the navigation-intent handoff on this branch.

The mechanism, all in
`frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx`
(line numbers at b2bcf0b) unless noted:

- The wheel handler walks the `large` subset (`pickNeighbourLargeMessage`,
  :1903) and commits the pick (:1919). The same-lane step path (:1649–1667)
  stamps `markProgrammaticScroll()` (:1661) then `scrollMessageIntoView`
  (:1662) — its ONLY guard against the follower is the 200 ms window
  (`PANE_SCROLL_PROGRAMMATIC_GUARD_MS`); there is no counter on this path.
- `article[data-message-uuid]` has
  `scroll-margin-top: var(--delta-top-region-reserve)` (index.css:469), and
  the reserve is > 0 whenever the timeline overlay exists. So
  `scrollIntoView({block:'start'})` parks the target `reserve` px below the
  container top, leaving the PREVIOUS article partially visible in the band
  above it.
- The follower picks the smallest `boundingClientRect.top` as topmost
  (:2251–2267, `threshold: 0` at :2298) — the previous article (top < 0,
  intersecting by a sliver) beats the target (top ≈ reserve). The commit goes
  through `setActiveMessageIndexFromPaneScroll` (:2275), which does not bump
  `scrubTick`, so ONLY the playhead moves; the pane stays put — matching the
  observed symptom exactly.
- The escape route past the time guard: the IO effect re-binds whenever
  `sortedMessages` identity or `activeThreadId` changes (deps :2345–2351;
  streaming ticks and background refetches change `sortedMessages` identity
  constantly in a live session). Every re-bind `observe()`s all articles
  (:2316–2326) and IO delivers an initial-observation batch → `flush`
  scheduled 100 ms later (:2294). If that lands more than 200 ms after the
  last `markProgrammaticScroll`, `flush` clears the guard and proceeds
  (:2233–2247) → commits topmost = the previous article → leftward yank.
  `MutationObserver` re-observes (:2330–2336) and resizes are further
  unguarded flush sources.

Fix — make the follower's commit IDEMPOTENT with respect to programmatic
scrolls, as a timing-independent invariant (per the design principle of the
prior fixes in this component; do NOT just widen the time window):

Select the article that owns the READING-REGION START LINE instead of the
raw smallest-top: among observed articles, pick the topmost one whose body
covers or starts below the reserve line (i.e. skip an article whose bottom
edge is at/above the reserve line; an article spanning the line is selected).
Since `scrollIntoView({block:'start'})` lands targets exactly at the reserve
line, any flush that escapes the guards then resolves to the SAME message the
scroll established — a no-op commit — so WHEN the flush fires no longer
matters. Read the reserve from the same source the CSS uses
(`--delta-top-region-reserve` on the container), falling back to 0 when
unset so existing jsdom tests keep their geometry. Apply the same selection
to every follower path (it also removes the same systematic off-by-one for
cross-lane landings once their counter guard releases).

Keep the existing guards (counter + 200 ms window) as-is: they still
suppress mid-flight noise; this fix removes the harm when they expire.

Existing suites that pin adjacent behavior (must stay green; update only if
their geometry needs to become reserve-aware): topmost-visible on pane
scroll (ThreadTimelineOverlay.test.tsx:3978), programmatic guard window
(:4063), cross-lane counter-guard suite (:4140 onward), and the
navigation-intent suites added on this branch.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit: after a same-lane wheel step parks the target at the reserve
      line (previous article partially visible above it), an IO flush
      arriving AFTER the programmatic-scroll guard window leaves the playhead
      on the target (fails at b2bcf0b: playhead commits the previous
      article).
- [x] Unit: a genuine user scroll that moves an earlier article's body into
      the reading region still moves the playhead to that article (the
      follower is not deadened).
- [x] Unit: an article spanning the reserve line (tall article whose top is
      above the line and bottom below it) is selected by the follower — no
      skip-ahead to the next article.
- [x] Unit: existing pinned suites still pass — topmost-visible on pane
      scroll, programmatic guard window, cross-lane counter guard, and the
      navigation-intent handoff suites from this branch.
- [x] e2e (mock mode, under `make e2e`): wheel-scrub the playhead rightward,
      idle past the guard window while mock content triggers a re-render —
      the playhead stays on the scrubbed mark (does not snap back left).

### Manual / on-hardware (verified by a human before merge)

- [x] In a real tool-heavy streaming session: wheel/arrow-scrub the playhead
      rightward, pause at various points — the playhead never snaps back
      left on its own.
- [x] Manually scrolling the transcript pane still drags the playhead to the
      message being read (follow behavior intact).
- [x] Cross-lane jumps, axis clicks, and navigator/left-pane selection
      behave as on the base branch (no regression to the two fixes under
      verification there).

## Out of scope

- Replacing the counter + 200 ms window pair with a navigation-generation
  token honored by IO flushes (recorded long-term direction; separate task).
- Suppressing observer initial/re-bind batches wholesale (only-follow-real-
  user-scroll); unnecessary once commits are idempotent, and it risks
  deadening legitimate follows.
- Filtering renders-nothing messages out of the timeline marks (dot⇄article
  contract; separate task).
