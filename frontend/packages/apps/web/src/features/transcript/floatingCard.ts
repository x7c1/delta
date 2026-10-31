/**
 * Shared chrome for the transcript pane's cards: the floating bottom notices
 * card and composer card that hover over the conversation, plus the in-flow
 * breadcrumb card that sits in the top region above it. A full border,
 * rounded corners, an opaque surface fill that occludes the conversation
 * beneath, and a shadow so the card reads as lifted above its surroundings
 * rather than fused to them. Per-card padding is applied at each use site.
 *
 * The shadow routes through `--delta-card-shadow` (rather than a literal
 * `shadow-md`) so the visual-effects gate can drop it under
 * `[data-effects='flat']` without touching functional overlay shadows; see the
 * token definition in src/index.css.
 */
export const FLOATING_CARD_CLASS =
  'rounded-md border border-border-default bg-surface shadow-(--delta-card-shadow)';
