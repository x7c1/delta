import type { ReactNode } from 'react';

/**
 * The chrome a rail item shares: a small box that RESTS ON the composer card's
 * top border rather than punching through it. No bottom border of its own, so
 * the card's top border runs beneath the item — which is what makes it read as
 * attached to the card, and what keeps the context-usage fill intact: that fill
 * rides the card's top border (see `composer-context-bar` in `TranscriptPane`),
 * so an item that overlapped downward in a thread would cover the very line it
 * sits on. The provider tabs (new-session mode only, where that fill can never
 * be present) are the one item that does reach 1px down, so their selected tab
 * can open into the card — see `ProviderTabs`. No shadow either — one would
 * fall onto the card's top face and break the "attached" read.
 */
export const COMPOSER_RAIL_ITEM_CLASS =
  'rounded-t-md border border-b-0 border-border-default bg-surface';

export interface ComposerRailProps {
  /**
   * The leftmost slot, holding the prompt-template button. That button renders
   * in BOTH composer modes, so in the app this slot is always filled — it is
   * what gives the rail its height in a thread, where there are no tabs.
   */
  templateSlot?: ReactNode;
  /**
   * The new-session provider tabs, or `null` in a thread (where the provider is
   * already fixed by the running session).
   */
  providerTabs?: ReactNode;
}

/**
 * The strip on the composer card's top edge: a thin, transparent lane that sits
 * OUTSIDE the card and carries small tab-like items resting on the card's top
 * border. It is the home for controls that belong to the card as a whole but
 * must not cost a row inside it or crowd the textarea.
 *
 * Rendered in normal flow directly above the card — never absolutely positioned
 * — because the bottom overlay measures its own height with a `ResizeObserver`
 * and drives the transcript body's bottom reserve from it. An out-of-flow rail
 * would not be counted, so the last turn would hide underneath it, and it would
 * overlap the notices card stacked above the composer.
 *
 * The rail itself draws nothing: the transcript shows through wherever it has
 * no item. With no items at all it collapses to zero height, but that is only
 * reachable in isolation — the app always fills `templateSlot`, so the rail
 * stands at one item's height in both composer modes and the composer card's
 * top edge never shifts as the tabs come and go.
 */
export function ComposerRail({
  templateSlot = null,
  providerTabs = null,
}: ComposerRailProps) {
  return (
    // `relative` makes the rail the anchor for panels an item opens (today the
    // prompt-template popover), instead of each item's own box. That box is only
    // a few pixels wide, so a panel measured against it can only be bounded by
    // the viewport — which ignores the navigator and terminal panes flanking
    // this one, and lets a wide panel spill out of the pane and under them. The
    // rail spans exactly the composer card, so anchoring here is what makes
    // "never wider than the card" expressible as `max-w-full`.
    <div className="relative flex items-end gap-1" data-testid="composer-rail">
      {templateSlot}
      {providerTabs}
    </div>
  );
}
