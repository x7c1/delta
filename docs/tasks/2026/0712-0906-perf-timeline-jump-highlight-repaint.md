---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check && ! grep -rn 'inset 0 0 0 9999px' frontend/packages/apps/web/src"
assignee: null
branch: task/0712-0906-perf-timeline-jump-highlight-repaint
created_at: 2026-07-12T09:06:00Z
updated_at: 2026-07-12T10:04:00Z
---

# perf(web): compositor-friendly timeline jump highlight

## Overview

Clicking a thread-timeline dot focuses the corresponding message and flashes
a landing highlight. The flash is implemented as an animated `box-shadow`
wash and is the main reason the jump feels janky, especially on WebKit
engines (observed in a WebKitGTK browser; WebKit is also what a future
Tauri shell would use on Linux/macOS — and even Chromium shows visible
stutter on long messages):

- `highlightMessageJump` in
  `frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx`
  forces a synchronous reflow (`void target.offsetWidth`) and then applies
  the `delta-timeline-jump-highlight` class for 800 ms.
- The matching CSS in `frontend/packages/apps/web/src/index.css` animates
  `box-shadow: inset 0 0 0 9999px rgb(var(--delta-color-highlight-wash) / 0.7)`
  down to alpha 0 (`@keyframes delta-timeline-jump-highlight-fade`).

Animating a huge-spread inset `box-shadow` is not compositor-accelerated in
any engine: the browser repaints the entire message element on every frame
for the full 800 ms. On a long assistant message that is a large paint area,
and on engines/setups without fast GPU rasterization the flash visibly
stutters and delays the perceived landing.

Replace the wash with a compositor-friendly equivalent that only animates
`opacity` (and/or `transform`), for example an absolutely-positioned overlay
element (or `::after` pseudo-element) filling the message article, painted
once with the highlight color and faded out via an `opacity` keyframe
animation. Requirements:

1. Visual result stays equivalent: a brief color wash over the landed
   message that fades out (duration/token reuse:
   `--delta-color-highlight-wash`, `TIMELINE_JUMP_HIGHLIGHT_MS`).
2. No `box-shadow`, `background-color`, or other paint-triggering property
   is animated for the flash; only `opacity`/`transform`.
3. Drop the forced synchronous reflow if the replacement mechanism does not
   need it (restarting a CSS animation can be done by re-adding the class
   across a frame boundary or animating a fresh overlay node).
4. Keep the existing `prefers-reduced-motion` behavior (no flash animation).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No animated huge-spread inset box-shadow remains:
      `grep -rn 'inset 0 0 0 9999px' frontend/packages/apps/web/src` returns
      no matches (appended to `check_command` as a gate).
- [x] The jump-highlight animation animates only `opacity`/`transform` in
      its keyframes (review of the replacement CSS).
- [x] `make check` passes (build, typecheck, tests, lint); tests that assert
      on the old class/keyframe names are updated.

### Manual / on-hardware (verified by a human before merge)

- [ ] Clicking a timeline dot still shows a clearly visible landing flash on
      the target message, in both light and dark themes.
- [ ] The flash is smooth (no visible stutter) on a long assistant message,
      checked in both a Chromium browser and a WebKit browser.
- [ ] With `prefers-reduced-motion: reduce`, no flash animation plays.

## Out of scope

- Virtualizing the transcript message list (separate, larger perf topic).
- Any change to how the timeline finds/scrolls to the target message
  (`scrollMessageIntoView`, cross-lane `scheduleScrollAfterRender`).
