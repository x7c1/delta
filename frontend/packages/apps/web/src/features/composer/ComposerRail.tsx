import type { ReactNode } from 'react';

/**
 * The chrome a rail item shares: a small box that RESTS ON the composer card's
 * top border rather than punching through it. No bottom border and no negative
 * margin, so the card's own top border runs uninterrupted beneath the item —
 * which is what makes it read as attached to the card, and what keeps the
 * context-usage fill intact: that fill rides the card's top border (see
 * `composer-context-bar` in `TranscriptPane`), so an item that overlapped
 * downward would cover the very line it sits on. No shadow either — one would
 * fall onto the card's top face and break the "attached" read.
 */
export const COMPOSER_RAIL_ITEM_CLASS =
  'rounded-t-md border border-b-0 border-border-default bg-surface';

export interface ComposerRailProps {
  /**
   * The leftmost slot, reserved for the prompt-template button. Nothing renders
   * it yet; the slot exists so the button lands at the rail's left end without
   * reshuffling what is already on it.
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
 * no item, and with no items at all (a thread) it collapses to zero height, so
 * nothing reserves space until there is something to put there.
 */
export function ComposerRail({
  templateSlot = null,
  providerTabs = null,
}: ComposerRailProps) {
  return (
    <div className="flex items-end gap-1" data-testid="composer-rail">
      {templateSlot}
      {providerTabs}
    </div>
  );
}
