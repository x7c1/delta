---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 2
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1016-0107-steady-the-prompt-template-popover-and-open-it-on-hover
created_at: 2026-10-15T16:07:00Z
updated_at: 2026-10-15T19:38:07Z
---

# feat(composer): keep the prompt-template popover a fixed size and open it on hover

## Overview

The prompt-template button sits at the left of the composer rail
(`frontend/packages/apps/web/src/features/composer/PromptTemplateButton.tsx`).
Clicking it opens `PromptTemplatePopover.tsx`: labels on the left, and the
hovered or focused template's full text on the right.

### The popover jumps under the pointer

The popover opens upward (`bottom-full`), so its bottom edge is fixed and its
top edge follows its height. Its height is the taller of the two columns, and
the preview column is as tall as the hovered template's text (capped at
`max-h-[50vh]`). Hovering a row with a longer or shorter template therefore
moves the top edge, the label column moves with it, and the row that was under
the pointer slides away — sometimes onto another row, whose hover changes the
height again.

Give the popover a height that does not depend on which template is
previewed: the list and preview columns share one fixed height (for example
`h-[min(50vh,<n>rem)]`, or the height of the tallest template, clamped), the
preview scrolls inside it, and the label column scrolls inside it too. Hovering
or arrowing through the list must never change the popover's box.

### Open on hover

Today only a click opens the popover. Open it when the pointer rests on the
button (a short intent delay, e.g. ~150 ms, so passing over the rail does not
flash it), and close it when the pointer has left both the button and the
popover (a short grace delay so the pointer can travel from the button into
the popover). While it is open by hover:

- Do not move focus into the popover — the user may be typing in the
  composer, and today's open path focuses the first row. Keep the caret in the
  textarea; the hovered row still drives the preview, and a click on a row
  inserts the template exactly as today.
- A click on the button while it is open by hover pins it open with today's
  keyboard behaviour (focus moves in, ↑/↓, Enter, Escape), and keyboard
  activation (Enter/Space on the focused button) opens it exactly as today.
- Escape and click-outside close it as today, in both modes.

Touch devices have no hover; a tap must keep working as the click it is today.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test renders the popover with templates of very different lengths and
      asserts the popover's height style/class does not change between hovering
      the shortest and the longest (no height derived from the previewed text).
- [x] Hovering the button opens the popover after the intent delay and does
      not move focus out of the textarea; leaving both the button and popover
      closes it after the grace delay; moving from the button into the popover
      keeps it open (fake timers).
- [x] Clicking a row of a hover-opened popover inserts the template at the
      textarea's caret as it stands at the click (the textarea keeps focus
      while the popover is open by hover, so the user may have typed since it
      opened; a caret captured at open would be stale).
- [x] A click or Enter on the button still opens it with focus on the first
      row, and ↑/↓/Enter/Escape behave as before (existing tests keep passing).

### Before merge (verified outside the check command)

- [x] `make mock` (or `make dev`) in a browser: with several templates of
      different lengths, hover the button, move down the list with the mouse,
      and confirm the popover stays put and the row under the pointer stays
      under it. The merging session checks this (screenshots or a short GIF)
      and records it in the PR.
