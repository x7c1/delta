---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 2
retries_remaining: 1
check_command: "make check && ! git grep -n 'composer-context-bar' -- frontend/packages"
assignee: null
branch: task/1016-0108-show-context-usage-as-a-meter-on-the-composer-rail
created_at: 2026-10-15T16:08:00Z
updated_at: 2026-10-15T20:02:02Z
---

# feat(composer): show context usage as a meter on the composer rail

## Overview

The focused session's context-window usage is drawn in
`frontend/packages/apps/web/src/features/transcript/TranscriptComposerStack.tsx`
(`composer-context-bar`): a 2px fill running along the composer card's top
border from the right, and a small `NN%` label overlaid on the card's top-right
corner. It was built before the composer rail existed, when there was no room
above the card, so it squeezes onto the card's edge.

The rail (`frontend/packages/apps/web/src/features/composer/ComposerRail.tsx`)
now gives the card a lane above it, with the prompt-template button on the
left. Move the usage indicator there:

- A rail item on the **right** end of the rail, the same height as the
  template button (22px; reuse `COMPOSER_RAIL_ITEM_CLASS` so it rests on the
  card's border the same way).
- Inside it, a fixed-width track with a bar that fills as usage grows, and the
  percentage beside it — the same visual language as the account rate-limit
  rows in the navigator footer (`RateLimitRow` in
  `features/navigator/NavigatorPane.tsx`, built on the ui-kit `Meter`):
  same fill token, same monospace tabular percentage, fill anchored the same
  way. Reuse `Meter` rather than drawing a new bar.
- Keep today's semantics: shown only when a snapshot exists (hidden, not 0%,
  before the first one and after `/compact`); the value is clamped to 0–100;
  the accessible `meter` role and label stay; the "Context window usage"
  tooltip stays, opening upward.
- Remove the old top-border fill and the overlaid label from the card, and
  the card-level wrapper and comments that only existed to protect them
  (`ComposerRail`'s doc comment explains that rail items must not overlap the
  card's top border to keep that fill visible — rewrite it for the new layout).
- Add a slot for right-aligned items to `ComposerRail` rather than positioning
  the meter absolutely; the rail must stay in normal flow (its doc comment
  explains why).
- New-session mode (provider tabs on the left, no session yet) shows no meter,
  as today; make sure the right slot being empty does not change the rail's
  height in either mode.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `git grep -n 'composer-context-bar' -- frontend/packages` finds nothing
      (appended to `check_command`); the card no longer renders a usage fill.
- [x] A test renders a thread with a usage snapshot and finds the meter inside
      the composer rail's right slot with `role="meter"`, the clamped
      `aria-valuenow`, and the rounded `NN%` text; values below 0 and above 100
      clamp.
- [x] With no snapshot the rail renders no meter and its height is unchanged
      (the template button still gives it its height).
- [x] In new-session mode the provider tabs render and no meter does.

### Before merge (verified outside the check command)

- [ ] `make mock` (or `make dev`) in a browser: in a thread with usage, the
      meter sits at the right end of the rail at the template button's height,
      the bar grows with the percentage, and nothing is drawn on the card's top
      border; the tooltip opens upward. The merging session checks this with a
      screenshot and records it in the PR.
