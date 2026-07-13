---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0713-0916-fix-branch-selection-dismiss-click
created_at: 2026-07-13T09:16:23Z
updated_at: 2026-07-13T10:26:00Z
---

# fix(web): dismiss pending branch selection on plain click regardless of selection-collapse timing

## Overview

Drag-selecting text in a transcript message arms a pending branch (the
"Branch from selected text" banner), and a plain click anywhere in the
transcript body is supposed to dismiss it. The dismissal listener in
`frontend/packages/apps/web/src/features/transcript/TranscriptPane.tsx`
(lines 446–462) gates on `window.getSelection()?.isCollapsed` at `click`
time: a click that leaves the selection collapsed clears the branch origin,
anything else is treated as the click that ends a drag-select and is ignored.
`frontend/packages/apps/web/src/features/transcript/MessageItem.tsx`
(lines 59–68) additionally re-arms the origin on any `mouseup` that sees a
non-empty selection.

That gate depends on *when the engine collapses the selection relative to the
`click` event*, and that timing is not something the app controls:

- **Verified cross-engine defect** (reproduced against the running app with
  Playwright, in both Chromium and WebKit): a plain click landing **on the
  selected text itself** leaves the selection un-collapsed through `mouseup`
  and `click` — the engine defers the collapse until after the click event.
  The guard therefore blocks dismissal, and the `mouseup` re-arm re-sets the
  origin. The banner never dismisses.
- **Engine-dependent cases** (verified in a minimal-page comparison): a click
  on a `<button>` keeps the selection alive in both engines; a click on a
  `user-select: none` region keeps it in Chromium but clears it in WebKit.
  Field reports from WebKit browsers (GNOME Web/WebKitGTK 2.52 on Linux and
  macOS Safari — engine builds that differ from Playwright's WebKit) show the
  deferred/skipped collapse covering enough click targets that "click
  anywhere to dismiss" effectively does not work there, while mostly working
  in Chromium.

Fix: stop inferring "was this the click that ended a drag-select?" from
selection state. Detect it directly:

1. Record the pointer position on `mousedown` (same `bodyRef` element the
   click listener is attached to).
2. On `click`, keep the pending branch iff the pointer moved beyond a small
   slop (a drag) **or** `event.detail > 1` (double/triple-click selection —
   those gestures just armed the origin via word/paragraph select).
3. Otherwise (a stationary single click, wherever it lands) dismiss: clear
   the branch origin, `clearBranchHighlight()`, and explicitly collapse the
   native selection (`getSelection().removeAllRanges()`) so engines that
   defer the collapse also drop the selection highlight instead of leaving
   stale selected text that the next `mouseup` would re-arm from.

No `MessageItem` change should be needed: on a stationary click over selected
text the `mouseup` re-arm still fires first, and the `click` dismissal then
clears it — the net result is dismissed. Note one deliberate behavior change:
a stationary click on an interactive element inside the transcript (e.g. a
branch chip or a details summary) now always dismisses the pending branch,
where today that depends on whether the engine collapsed the selection for
that target. This is the intended reading of "a plain click anywhere in the
transcript body dismisses".

Update the two existing unit tests built around the `isCollapsed` mock in
`frontend/packages/apps/web/src/features/transcript/TranscriptPane.test.tsx`
(lines 1160–1208) to the new semantics, and add an e2e spec (chromium
project, mock mode) that drives the real mouse gestures.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit (`TranscriptPane.test.tsx`): a stationary single click whose
      `window.getSelection()` still reports a non-collapsed selection
      (WebKit-style deferred collapse) clears the pending branch origin and
      collapses the native selection.
- [x] Unit: a click preceded by pointer movement beyond the slop (drag-select
      end) keeps the pending branch origin, without relying on a mocked
      `isCollapsed: false`.
- [x] Unit: a click with `detail > 1` (double/triple-click selection) keeps
      the pending branch origin.
- [x] e2e (new spec, runs under `make e2e` which is part of
      `check_command`): drag-selecting message text shows the "Branch from
      selected text" banner; a plain click on a textless gap between messages
      dismisses it; re-selecting and plain-clicking **directly on the
      selected text** also dismisses it (regression test for the confirmed
      cross-engine failure).

### Manual / on-hardware (verified by a human before merge)

- [ ] GNOME Web (Epiphany/WebKitGTK) on Linux: select message text, then
      click an arbitrary transcript spot (empty area, other text, and the
      selected text itself) — the banner and the selection highlight are
      dismissed in each case.
- [ ] macOS Safari: same checks as above.
- [ ] Chromium: drag-select still arms the banner without it being dismissed
      by the release click, and double-click word-select arms it and
      survives its own click.

## Out of scope

- Adding a WebKit project to the Playwright e2e suite (engine-compat CI
  infrastructure is a separate decision).
- Changing how the branch origin is armed from a selection (`MessageItem`
  mouseup semantics) beyond what the dismissal fix requires.
- The unrelated intermittent timeline-playhead follow bug observed in
  dogfooding.
