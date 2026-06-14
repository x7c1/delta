/**
 * Auto-grow geometry for the composer textarea, kept as a pure module so the
 * clamp can be unit-tested without a DOM (jsdom does not lay out content, so a
 * real `scrollHeight` is never available under test).
 *
 * The composer grows with its content up to a cap, then scrolls internally:
 * the height is the content height (`scrollHeight`) clamped to `[min, max]`, and
 * once the content exceeds the cap the textarea must show its own scrollbar
 * (`overflow-y: auto`) instead of growing further. Below the cap the scrollbar
 * is hidden (`overflow-y: hidden`) so a non-overflowing textarea never flickers
 * a bar while it grows.
 */

/** Min textarea height (~2 rows), matching the resting composer size. */
export const COMPOSER_MIN_HEIGHT = 40; // px (≈ 2.5rem)

/**
 * Max textarea height before it scrolls internally (~7 lines). Past this the
 * textarea stops growing and scrolls, so a long draft never pushes the
 * conversation tail out of view.
 */
export const COMPOSER_MAX_HEIGHT = 160; // px (10rem)

export interface AutoGrowGeometry {
  /** The height to apply, clamped to [min, max]. */
  height: number;
  /** Whether the content overflows the cap (so the textarea must scroll). */
  overflow: boolean;
}

/**
 * Resolve the textarea height and overflow from its measured content height.
 *
 * @param scrollHeight the textarea's content height (its `scrollHeight` after
 *   its inline height has been reset to `auto`).
 * @param min minimum height (defaults to {@link COMPOSER_MIN_HEIGHT}).
 * @param max cap before internal scrolling (defaults to
 *   {@link COMPOSER_MAX_HEIGHT}).
 */
export function autoGrowGeometry(
  scrollHeight: number,
  min: number = COMPOSER_MIN_HEIGHT,
  max: number = COMPOSER_MAX_HEIGHT,
): AutoGrowGeometry {
  const height = Math.max(min, Math.min(scrollHeight, max));
  // Only show the scrollbar once the content genuinely exceeds the cap, so a
  // textarea that fits never flashes a bar as it grows toward the cap.
  const overflow = scrollHeight > max;
  return { height, overflow };
}
