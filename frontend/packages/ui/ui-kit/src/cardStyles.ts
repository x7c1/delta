/**
 * The class strings shared by the two card shapes — {@link Collapsible} (its
 * body opens on click) and {@link Card} (its body is always shown). They are
 * kept here rather than duplicated so the interactive and the static card
 * cannot drift apart visually.
 */

/** The bordered, elevated frame both cards sit in. */
export const CARD_FRAME_CLASS =
  'rounded border border-border-default bg-surface-elevated';

/**
 * The one-line caption row above the body. `Collapsible` renders it as a
 * button and adds its own `w-full text-left` plus a hover background.
 */
export const CARD_CAPTION_CLASS =
  'flex items-center gap-1 px-2 py-1 text-caption text-fg-muted';

/** The body below the caption, separated by a divider. */
export const CARD_BODY_CLASS =
  'border-t border-border-default px-2 py-1.5 text-caption';
