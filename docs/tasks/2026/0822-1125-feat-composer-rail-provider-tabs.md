---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0822-1125-feat-composer-rail-provider-tabs
created_at: 2026-08-22T11:25:00Z
updated_at: 2026-08-22T12:21:10Z
---

# feat(web): add a composer top rail and move the provider selector onto it as tabs

## Overview

Introduce a **composer top rail**: a thin strip that sits on the composer
card's top edge, *outside* the card, carrying small tab-like elements. It is
the home for controls that belong to the card as a whole but must not cost a
row inside the card or crowd the textarea. The first occupant is the
new-session provider selector (Claude | Codex), reshaped as tabs; a later task
adds a prompt-template button at the rail's left end. This task builds the
rail and moves the provider selector — it does not add the template button.

```
thread mode (rail empty in this task; the left slot is reserved)
┌──────────────────────────────────────┐
│ Message #main…                  42% │
│                                 (➤) │
└──────────────────────────────────────┘

new-session mode
      ┌────────┬────────┐
      │ Claude │ Codex  │
┌─────┴────────┴────────┴──────────────┐
│ ~/src/foo  ✎                         │  ← WorkdirChip / WorktreeOptions / LaunchOptionsPicker stay inside the card
│ Message to start a new session…      │
│                                 (➤) │
└──────────────────────────────────────┘
```

### The rail

- Rendered in `TranscriptPane.tsx` as part of the bottom overlay: wrap the
  composer card (`data-testid="composer-card"`,
  `frontend/packages/apps/web/src/features/transcript/TranscriptPane.tsx`
  around `:1070-1140`) in a `flex-col` container and place the rail as a
  **normal-flow** element directly above the card. **Do not absolutely
  position it.** The bottom overlay measures its own height with a
  `ResizeObserver` and drives the transcript body's bottom padding from it
  (`TranscriptPane.tsx:1145-` and the measure effect below it); an
  absolutely-positioned rail would not be counted, so the last turn would be
  hidden under it, and with the notices card present (`bottom-notices`, 8px
  `gap-2` above the composer) the rail would overlap the notices card. In
  normal flow the rail's height is included in the measurement and the
  `gap-2` stays above the rail.
- The rail is transparent except for its items; the transcript shows
  through the empty part. Items are small boxes that **rest on the card's top
  border**: they have left, top and right borders (`border-border-default`),
  `rounded-t-md`, `bg-surface`, no bottom border, and **no negative margin**
  — the card's top border runs uninterrupted beneath them. This matters for
  the context-usage fill (`composer-context-bar`, a 2px fill along the card's
  top border from the right edge): the rail must sit strictly *above* that
  line and never cover it. No shadow on rail items (a shadow would fall onto
  the card's top face and break the "attached" read).
- Layout: a horizontal `flex` row, `items-end`, left-aligned. Reserve the
  leftmost slot for the future template button (a named placeholder slot in
  the component — no visible element in this task) and render provider tabs
  to its right with a small gap. In thread mode the rail has no items; it may
  then render with zero height (no reserved strip), since nothing needs the
  space until the template button task lands.
- Put the rail in its own component under `features/composer/` (e.g.
  `ComposerRail.tsx`) with `data-testid="composer-rail"`, documented as
  "the strip on the composer card's top edge" — call the strip a *rail*, the
  provider control *tabs*; do not describe every rail item as a tab.

### Provider selector → tabs

- Move `ProviderSelector`
  (`frontend/packages/apps/web/src/features/composer/ProviderSelector.tsx`)
  out of the card's flow (it is currently the first child of the card's
  `space-y-2` stack, `TranscriptPane.tsx:1126`) onto the rail, new-session
  mode only. Keep its behavior and accessibility contract intact: still a
  `role="radiogroup"` of visually-hidden native radios (`sr-only`), still
  `data-testid="provider-selector"` and `provider-option-<provider>` per
  segment, still seeding from the persisted default, availability gating,
  fail-open, fallback off an unavailable provider. Restyle the segments as
  adjacent tabs on the rail: selected tab `bg-surface text-fg font-medium`,
  unselected `bg-surface-elevated text-fg-muted hover:text-fg`; disabled
  (unavailable) tab `opacity-50 cursor-not-allowed` with the server reason
  in `title`. The selected tab is distinguished by color/weight only — it
  does **not** erase the card's top border beneath it (see the
  context-fill rule above).
- The unavailable-provider notice (`provider-unavailable-notice`,
  `ProviderSelector.tsx` tail) does not fit on the rail; keep rendering it
  **inside the card** as the first child of the `space-y-2` stack, so the
  explanation stays where it is today. Split the component accordingly
  (tabs on the rail, notice in the card) while sharing one availability
  computation.
- `ProviderName` with its hue tint stays the tab label.
- Update `ProviderSelector.test.tsx` for the new structure (the nine existing
  behaviors must keep passing, re-targeted as needed), and any other test
  that locates the selector (`PRTab.test.tsx`, e2e specs that click
  `provider-option-*`).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] In new-session mode the provider tabs render on the rail
      (`composer-rail` contains `provider-selector`), and the card's
      `space-y-2` stack no longer contains the selector; in thread mode the
      rail renders without provider tabs (component / integration tests on
      `TranscriptPane` or its test harness).
- [x] The rail is in normal flow: the bottom overlay's measured height grows
      by the rail's height in new-session mode (test that the element is a
      flow child of the overlay — not `position: absolute` — and that the
      body's bottom reserve reflects it), and the notices card, when present,
      sits above the rail with the existing gap.
- [x] Rail items never cover the context-usage fill. The fill renders only
      in thread mode (`contextUsage` needs an active thread), where the rail
      has no items and collapses to zero height; in new-session mode every
      rail item rests on the card's top border with `border-b-0` and no
      negative bottom margin (structural assertions in component tests —
      the two cannot co-occur in one render, so there is no pixel-row
      geometry test; the tabs' bottom edge = card top edge was confirmed in
      a real browser during refine).
- [x] All nine existing `ProviderSelector.test.tsx` behaviors pass against
      the tab rendering: both providers as radios with hue-tinted names;
      default Claude checked; picking writes to the store; seeding from the
      persisted default; no re-seed; explicit pick preserved; unavailable
      provider disabled with the server reason; disabled not selectable;
      fallback off an unavailable default.
- [x] The unavailable-provider notice still renders inside the card
      (`provider-unavailable-notice` is a descendant of `composer-card`, not
      of `composer-rail`) (component test).
- [x] Every existing spec that drives `provider-option-*` (e2e and e2e-fake)
      passes unchanged or with selector-only edits.
- [x] `make check` passes.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app (new-session), the Claude | Codex tabs read as tabs
      resting on the card's top edge with the card border continuous beneath
      them; switching provider feels like switching tabs; the selected tab
      is unmistakable on light, dark and sepia themes.
- [ ] In a thread with context usage shown, the top-border fill and the `NN%`
      readout look exactly as before (nothing new touches that edge).
- [ ] Typing a long message (auto-grow) and the appearance of the pending
      strip / notices card above the composer keep the rail attached to the
      card with no overlap or jump.

## Out of scope

- The prompt-template button and popover (separate task; this task only
  reserves the rail's left slot).
- Moving WorkdirChip, WorktreeOptions or LaunchOptionsPicker out of the card
  — they can be multi-line and conditional, so they stay inside.
- Any change to provider availability semantics or the default-provider
  setting.
