---
status: completed
pipeline_phase: null
plan: null
base_ref: task/0714-0558-fix-timeline-playhead-navigator-follow
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0714-0848-fix-timeline-playhead-realign-clobber
created_at: 2026-07-14T08:48:45Z
updated_at: 2026-07-14T12:33:32Z
---

# fix(web): stop the timeline realign effect from reverting a committed playhead move

## Overview

Follow-up to the playhead-follow fix on branch
`task/0714-0558-fix-timeline-playhead-navigator-follow` (this task stacks on
it). On-hardware testing showed the user-visible symptom persists: selecting a
thread or child thread from the left-pane session list still leaves the
timeline playhead on the previous thread. An instrumented reproduction
(deterministic 3/3 in mock mode, Chromium) pinned the real mechanism, which is
neither the guard-counter latch fixed previously nor an overlay remount:

The external-thread effect's reposition **does commit**, and is then reverted
by the index-preservation ("realign") effect at
`frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx:1200–1225`.
Trace: left-pane selection changes `activeThreadId`; WorkspaceScreen's binding
flush runs `invalidateThreadMessages` (WorkspaceScreen.tsx:269–273), so a
`messages` refetch — and therefore a new `sortedMessages` array identity — is
guaranteed moments after every selection. The realign effect re-resolves the
active index from `activeMessageRef.current?.uuid`, and when its effect pass
runs before the ref-sync effect (:1164–1167) has caught up with the just-
committed reposition, the ref still points at the **previous thread's
message**; `findIndex` resolves it and the setter enqueues after the
reposition's updater, so the stale index wins. Probe log:

    [ext-effect] COMMIT {targetUuid: uuid-b3b, targetIndex: 7}
    [fromPaneScroll] index 0 -> 7
    [realign] index 7 -> 0 refUuid uuid-u1     ← stale-ref revert
    [render] idx 0 uuid uuid-u1                ← playhead stranded

Whether the refetch commit outraces the reposition's render is a React
scheduling race — deterministic in mock mode, a coin flip against real backend
latency — which is exactly why the bug reads as intermittent on hardware.

Two aggravating facts, confirmed by the same probes:

- The IntersectionObserver "self-heal" cannot mask the revert: the flush
  (:2023–2083) drops batches while `crossLaneJumpInFlightCountRef > 0` or
  inside the 200 ms programmatic-scroll window — both raised by the external
  effect's own commit (:1427–1428) — and dropped batches are not re-armed
  (:2098–2101), so on an idle thread no later batch arrives. When a flush does
  fire it anchors to the topmost-visible article (:2058–2074), not the lane's
  latest large turn.
- On a genuinely fresh overlay mount, `externalThreadInitializedRef` consumes
  the initial `activeThreadId` without repositioning (:1348–1351) and the
  auto-anchor (:1178–1189) lands on the global tail across all lanes — briefly
  highlighting the wrong lane — until IO moves it. Remount on selection is
  rare (it needs a cold threads-query cache), but the wrong initial anchor is
  observable on every fresh expand.

Fix, in two parts (both pure state-machine changes; no timers, no
engine-dependent timing):

1. **Make the active message UUID the canonical state and derive the index per
   render.** Store the active UUID (plus the existing tick bookkeeping);
   compute the index with `useMemo` over `sortedMessages`; delete the realign
   effect and `activeMessageRef` entirely. An array-identity change can then
   never move the playhead, by construction. Wheel/keyboard step handlers
   (:1699, :1799) and other index consumers switch to the derived index. If
   during implementation this proves to have an unacceptably large blast
   radius, the fallback is the minimal variant — every committer of
   `activeMessageIndexState` (setActiveMessageIndex :1090–1101,
   setActiveMessageIndexFromPaneScroll :1119–1141, and the external effect's
   commit :1429) synchronously writes `activeMessageRef.current` at commit
   time so realign always resolves the newest committed intent — but the
   structural variant is strongly preferred.
2. **Make overlay mount a reposition, not a skip.** Replace the
   initial-consume at :1348–1351 with: on mount (and whenever the lane's
   messages later load — reuse the existing retry-via-`largeSortedMessages`
   pattern), anchor the playhead to `activeThreadId`'s latest large turn; fall
   back to the global tail only when `activeThreadId` is null. This closes the
   remount corner case for good and fixes the wrong-lane flash on first
   expand. Update the unit tests that pin the mount-lands-on-global-tail
   behavior (ThreadTimelineOverlay.test.tsx :392, :846, :894, :2023, :2162,
   :4104, :5753, :5782, :6209 — the :5753/:5782 comments explicitly call the
   old behavior an auto-anchor pick, not a product requirement) to expect the
   active lane's latest large turn.

Also reroute the e2e (`e2e/timeline-playhead-follow.spec.ts`) thread
selection through the **left-pane session list** — the user-facing path it
previously had to avoid precisely because of this bug — and keep the
renders-nothing/timeout scenario intact.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit: after an external `activeThreadId` reposition commits, replacing
      `sortedMessages` with a new array identity (same or superset content,
      e.g. simulating the post-selection messages refetch) leaves the playhead
      on the reposition target (fails before this fix: stale-ref realign
      revert).
- [x] Unit: a `sortedMessages` identity change that appends messages to a
      non-active lane does not move the playhead off the active message.
- [x] Unit: on first mount with a non-null `activeThreadId`, the playhead
      anchors to that thread's latest large turn — retrying when the lane's
      messages load later — and anchors to the global tail only when
      `activeThreadId` is null (updated mount-anchor expectations across the
      pinned tests listed in the Overview).
- [x] e2e (mock mode, under `make e2e`): selecting a sibling/child thread from
      the left-pane session list moves the playhead and active-lane highlight
      to the selected thread (the reproduced clobber scenario, previously
      failing deterministically in mock mode).
- [x] e2e: the existing renders-nothing cross-lane timeout scenario still
      passes with thread selection routed through the left-pane session list
      instead of the transcript breadcrumb.

### Manual / on-hardware (verified by a human before merge)

- [x] In a real dogfooding session with tool-heavy threads: interleave
      timeline axis clicks with thread and child-thread selections from the
      left-pane session list, and confirm the playhead and lane highlight
      follow every selection.
- [x] Wheel/arrow timeline scrubbing and cross-lane jump clicks still land and
      animate as before.
- [x] Expanding the timeline fresh (first open on a session) anchors the
      playhead to the active thread's lane, not another lane's tail.

## Out of scope

- Replacing the in-flight counter + 200 ms programmatic-scroll window with a
  navigation-generation token carried by IO flushes (would also let suppressed
  flushes re-arm instead of dropping, and retire the topmost-visible-vs-
  latest-turn mismatch). Right long-term direction; separate task.
- Filtering renders-nothing messages out of the timeline marks (dot⇄article
  contract), unchanged from the previous task's out-of-scope list.
