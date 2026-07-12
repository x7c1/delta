---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0712-1035-perf-composer-autosize-reflow
created_at: 2026-07-12T10:35:00Z
updated_at: 2026-07-12T14:44:00Z
---

# perf(web): stop the composer autosize from forcing a full relayout per keystroke

## Overview

Typing in the message composer visibly lags on WebKit engines (observed in a
WebKitGTK browser; WebKit is also what a future Tauri shell would use on
Linux/macOS), and the lag grows with the length of the open thread. The
React side is already well-scoped — the draft lives in the zustand composer
store with `Composer` as its only subscriber, so a keystroke re-renders only
the composer, and no per-keystroke persistence or network work exists. The
cost is layout, not React:

- `frontend/packages/apps/web/src/features/composer/Composer.tsx` has a
  `useLayoutEffect` keyed on `[draft]` that runs on **every keystroke**: it
  sets `el.style.height = 'auto'` (invalidating layout), then reads
  `el.scrollHeight` — a **forced synchronous reflow before paint** (the
  clamp itself is in `autoGrow.ts`).
- The transcript is not virtualized: every message of the open thread, with
  fully rendered Markdown, shares the textarea's layout tree. The forced
  reflow therefore re-lays-out the entire live DOM, so its cost scales with
  thread length and is paid synchronously on each keystroke — which is
  exactly the reported symptom, and is amplified on WebKitGTK's slower
  layout path.

Keep the autosize behavior (the textarea grows and shrinks with content, up
to its existing clamp) but stop paying a full-document reflow on keystrokes
that cannot change the height. Candidate directions — pick the simplest one
that measurably removes the per-keystroke reflow, and prefer localizing
layout over throttling it:

1. **Contain the layout scope**: give the textarea's measurement a locally
   scoped layout (e.g. `contain: layout` / `content-visibility` on suitable
   ancestors, or measuring a hidden absolutely-positioned mirror element
   that mirrors width + content) so the `scrollHeight` read no longer walks
   the transcript's layout tree.
2. **Measure less often**: skip the `height:'auto'` + `scrollHeight` cycle
   when the height cannot have changed (cache the last measured content
   height and only re-measure when the value's line count / wrap-relevant
   width changes, or coalesce measurements into one `requestAnimationFrame`
   per burst of keystrokes so intermediate keystrokes skip layout).
3. CSS `field-sizing: content` would delete the JS entirely but is not yet
   available in WebKit — it may inform the design (progressive enhancement)
   but cannot be the cross-engine fix.

Whatever the approach, the visible behavior must not regress: growing while
typing multi-line content, shrinking when content is deleted, the existing
max-height clamp, and correct sizing after programmatic draft changes
(quote insertion, draft restore on thread switch).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes (build, typecheck, tests, lint), including any
      updated composer tests.
- [x] Composer unit tests cover the autosize behavior that remains
      JS-driven: grows with multi-line input, shrinks on deletion, respects
      the max-height clamp, and resizes after a programmatic draft change
      (quote insertion / thread switch).

### Manual / on-hardware (verified by a human before merge)

- [x] With a long thread open (50+ messages with Markdown), typing in the
      composer feels responsive in a WebKit browser — no per-keystroke
      hitching; compare against Chromium for parity. Verified on-hardware:
      clearly improved over the previous build; WebKit still trails
      Chromium slightly on a long thread, consistent with the remaining
      out-of-scope multiplier (non-virtualized transcript).
- [x] A performance profile (or equivalent observation) confirms keystrokes
      no longer trigger a full-document synchronous reflow from the
      composer autosize path. Verified structurally (measurement moved off
      the keypress-to-paint path into one coalesced rAF) and by the felt
      improvement above.
- [x] Autosize still behaves correctly end-to-end: grow, shrink, clamp,
      quote insertion, and draft restore when switching threads.

## Out of scope

- Virtualizing the transcript message list (separate, larger perf topic —
  it is the multiplier here, but the composer must not force full-document
  layout regardless).
- Switching the textarea to an uncontrolled component or moving the draft
  out of the zustand store (the React round-trip is not the bottleneck).
