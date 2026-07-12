import { useEffect, type RefObject } from 'react';
import { autoGrowGeometry } from './autoGrow';

/**
 * Auto-grow a controlled textarea to fit its content, coalescing the
 * measurement into a single animation frame per burst of changes so typing
 * never forces a synchronous reflow before paint.
 *
 * The measurement itself — reset the inline height to `auto`, then read
 * `scrollHeight` — is a forced layout flush: the write invalidates layout and
 * the read makes the browser lay out synchronously to answer it. Doing that in
 * a `useLayoutEffect` on every keystroke meant each character had to wait on a
 * relayout of everything sharing the textarea's layout tree (the whole open
 * transcript, with its rendered Markdown), before it could paint — the cost
 * grew with thread length and was badly amplified on WebKit's slower layout
 * path, which is the reported per-keystroke hitch.
 *
 * Deferring the measurement to `requestAnimationFrame` and cancelling any frame
 * already queued for a previous change means:
 *
 * - the keystroke's committed value paints immediately, with no forced reflow
 *   sitting between keypress and paint, and
 * - a burst of keystrokes arriving within one frame collapses to a single
 *   measurement (the earlier frames are cancelled), so intermediate keystrokes
 *   skip layout entirely.
 *
 * The accepted cost is a one-frame flash, but only on a genuine mount: a
 * Composer that first appears already holding a multi-line draft (e.g. a
 * persisted draft restored on page load) paints once at the min height before
 * the next frame grows it, whereas the old synchronous `useLayoutEffect` sized
 * it ahead of the first paint. A thread switch does not hit this path —
 * `TranscriptPane` keeps the same Composer instance across threads (no `key`),
 * so switching only changes `value` on an already-mounted node. This is the
 * deliberate trade-off for dropping the per-keystroke reflow.
 *
 * The clamp/overflow policy stays in the pure {@link autoGrowGeometry} so it can
 * be unit-tested without a DOM.
 *
 * `value` is the controlled draft: the effect re-runs — scheduling a frame —
 * whenever it changes, so the textarea also resizes after programmatic edits
 * (draft restore on a thread switch, quote insertion), not only while typing.
 * The `maxHeight` cap is also set inline on the element as a hard ceiling, so
 * the textarea can never overshoot during the one frame before the measurement
 * settles it.
 */
export function useAutoGrow(
  ref: RefObject<HTMLTextAreaElement | null>,
  value: string,
): void {
  useEffect(() => {
    const el = ref.current;
    if (!el) {
      return;
    }
    const frame = requestAnimationFrame(() => {
      // Reset to `auto` first so `scrollHeight` reflects the content's natural
      // height (not a previously-applied larger one), then clamp it and toggle
      // the internal scrollbar past the cap.
      el.style.height = 'auto';
      const { height, overflow } = autoGrowGeometry(el.scrollHeight);
      el.style.height = `${height}px`;
      el.style.overflowY = overflow ? 'auto' : 'hidden';
    });
    return () => cancelAnimationFrame(frame);
  }, [ref, value]);
}
