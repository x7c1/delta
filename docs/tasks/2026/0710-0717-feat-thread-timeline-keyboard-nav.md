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
branch: task/0710-0717-feat-thread-timeline-keyboard-nav
created_at: 2026-07-10T07:17:00Z
updated_at: 2026-07-10T08:57:00Z
---

# feat(web): keyboard navigation for the thread timeline playhead

## Overview

The thread timeline overlay
(`frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx`)
currently offers three ways to move the playhead: wheel scrub over the
axis, click-to-jump on an axis cell, and the two jump-to-edge buttons.
On a Mac trackpad the wheel path is unreliable for precise navigation:
even with the output-side commit cooldown (`WHEEL_STEP_COOLDOWN_MS`,
delta#167) bounding throughput, the inertial event stream keeps feeding
steps after the fingers lift, so stopping on an intended message is a
matter of luck. A keyboard step is fully deterministic — one keypress,
one step — which is exactly the guarantee the trackpad cannot give.

Add ArrowLeft / ArrowRight keyboard navigation to the expanded timeline:

- **ArrowLeft** steps the playhead one large message towards the older
  end (timeline left); **ArrowRight** one large message towards the
  newer end (timeline right). The direction mapping mirrors the visual
  axis (left = past, right = latest) and the wheel convention
  (positive delta = newer).
- **Step semantics are identical to one wheel step**: walk the
  large-message subset with the existing `pickNeighbourLargeMessage`
  helper, starting from `activeMessageIndexRef.current ?? total - 1`,
  and commit through the existing `setActiveMessageIndex` so the
  scrub-tick bump, the conversation-pane jump effect, and cross-lane
  active-thread switching all behave exactly as they do for wheel
  scrubs. Clamp at the ends — no wrap.
- **One keydown = exactly one step.** Do NOT apply
  `WHEEL_STEP_COOLDOWN_MS` and do not use the velocity-window /
  staircase machinery: keys are a deterministic input whose cadence is
  set by the OS key-repeat, and holding the key is the intended way to
  traverse quickly. `event.repeat` events step like any other keydown.
- **Listener lifecycle**: a `window`-level `keydown` listener attached
  in an effect that is active only while the overlay is `expanded`
  (mirroring how the wheel listener is scoped to the expanded axis
  container). Collapsed timeline → no listener, keys fall through to
  the page untouched.
- **Guards — the listener must NOT interfere with text entry or the
  terminal.** Return without calling `preventDefault` when:
  - the key is anything other than plain ArrowLeft / ArrowRight
    (any of `ctrlKey` / `metaKey` / `altKey` set → not ours; native
    shortcuts like Alt+Arrow word-jump keep working),
  - `event.defaultPrevented` is already set,
  - the event target is editable: `input`, `textarea`, `select`, or
    anything inside an `isContentEditable` element. This covers the
    composer textarea AND xterm's hidden helper textarea
    (`.xterm-helper-textarea` in `TerminalPane`), so arrow keys typed
    into the terminal are never hijacked.
- When the guards pass and the key is ArrowLeft/ArrowRight, always call
  `event.preventDefault()` (even when the step clamps into a no-op at
  either end) so the keypress never leaks into page-level horizontal
  scrolling while the timeline owns it.

There is no global keyboard-shortcut registry in the web app today
(existing `keydown` handling is all element-local: composer, settings
rail, workdir picker); introducing one is out of scope — attach the
listener locally in `ThreadTimelineOverlay` the same way the wheel
listener is attached.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] With the timeline expanded, a plain ArrowRight keydown on
      `window` moves the playhead to the next large message and
      ArrowLeft to the previous one, committed through the existing
      jump path (component test observes the same active-mark /
      playhead change a wheel step produces).
- [x] Steps walk the large-message subset: with small (auxiliary)
      marks between two large marks, one keypress lands on the
      adjacent large message, skipping the small ones.
- [x] Key-repeat events step once each (a burst of N ArrowRight
      keydowns with `repeat: true` advances N large messages, no
      cooldown), and the playhead clamps without wrapping at both
      ends.
- [x] Guarded events do not move the playhead and do not get
      `preventDefault`-ed: keydowns targeting an `input` / `textarea` /
      `select` / contentEditable element, keydowns with Ctrl / Meta /
      Alt modifiers, and keydowns whose `defaultPrevented` is already
      set.
- [x] Handled ArrowLeft/ArrowRight keydowns are `preventDefault`-ed,
      including at the clamped ends.
- [x] While the timeline is collapsed, no keydown listener is active
      (arrow keydowns neither move state nor get `preventDefault`-ed).
- [x] `make check` passes (backend build/test/clippy + generated
      bindings freshness + frontend build/typecheck/test/lint).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a MacBook trackpad setup: with the timeline expanded,
      ArrowLeft/ArrowRight reliably move the playhead exactly one
      message per press — the precision that wheel scrubbing lacks —
      and holding the key traverses at the OS repeat rate.
- [ ] Typing in the composer (including arrow-key caret movement) and
      arrow-key input into the terminal pane are unaffected while the
      timeline is expanded.

## Out of scope

- A global keyboard-shortcut registry / help overlay for the web app.
- Home/End (jump-to-edge) key bindings — the header jump buttons
  already cover one-shot edge navigation.
- Any change to the wheel scrub tuning (`WHEEL_STEP_COOLDOWN_MS`,
  staircase, velocity window).
