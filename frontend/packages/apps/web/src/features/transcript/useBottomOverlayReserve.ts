import {
  useLayoutEffect,
  useRef,
  useState,
  type MutableRefObject,
} from 'react';
import { useComposerStore } from '../../store/composerStore';

/**
 * Fallback gap (px) between the bottom overlay and the resting content when the
 * `--delta-overlay-inset` token cannot be read (e.g. jsdom, which computes no
 * styles). Mirrors the token's resting value (0.75rem at the 16px default root)
 * so the measured reserve still leaves the same gap the floating card uses.
 */
const OVERLAY_INSET_FALLBACK_PX = 12;

/**
 * Comfortable reading gap (px) kept between the last turn and the bottom overlay,
 * added on top of the measured reserve. The measured reserve (overlay height +
 * inset) alone parks the last turn flush against the composer, which reads worse
 * than leaving some air there — so a slice of the breathing room the old fixed
 * reserve always had is preserved here while the rest of the reserve still tracks
 * the overlay's real height. Present at every composer size and grows with it.
 */
const BODY_BOTTOM_READING_GAP_PX = 192;

/**
 * The overlay inset in pixels: the gap the floating cards leave from the body
 * edges (`--delta-overlay-inset`). Read from the live computed style so the
 * measured bottom reserve (overlay height + this gap) stays in lockstep with the
 * card's own `bottom-overlay-inset`, even if the token is themed. Falls back to
 * {@link OVERLAY_INSET_FALLBACK_PX} when the value is unavailable or non-numeric.
 */
function overlayInsetPx(el: Element): number {
  const raw = getComputedStyle(el)
    .getPropertyValue('--delta-overlay-inset')
    .trim();
  if (raw.endsWith('rem')) {
    const rem = Number.parseFloat(raw);
    const rootSize = Number.parseFloat(
      getComputedStyle(document.documentElement).fontSize,
    );
    const px = rem * (Number.isFinite(rootSize) ? rootSize : 16);
    return Number.isFinite(px) ? px : OVERLAY_INSET_FALLBACK_PX;
  }
  const px = Number.parseFloat(raw);
  return Number.isFinite(px) ? px : OVERLAY_INSET_FALLBACK_PX;
}

interface BottomOverlayReserveArgs {
  /** Whether a bottom overlay is mounted at all. */
  hasBottomOverlay: boolean;
  /** The scrolling transcript body. */
  bodyRef: MutableRefObject<HTMLDivElement | null>;
  /** Whether the body is following new content. */
  stickRef: MutableRefObject<boolean>;
}

/**
 * Measure the floating bottom overlay (composer stack) and derive the body's
 * bottom padding from its height. Returns the ref the overlay binds and the
 * measured reserve, `null` until measured.
 */
export function useBottomOverlayReserve({
  hasBottomOverlay,
  bodyRef,
  stickRef,
}: BottomOverlayReserveArgs) {
  // The bottom overlay (composer + send-activity strip + bottom notices) floats
  // over the scrolling body and grows with the composer's content. Reserve
  // bottom padding equal to its MEASURED height (plus the overlay inset as a
  // gap) so the last turn always rests just above it and stays readable as it
  // grows — replacing the old fixed `pb-composer-reserve`, which a grown
  // composer would cover. `null` until measured: the body falls back to the
  // fixed reserve so a first paint (or a body without an overlay) never
  // under-reserves.
  const bottomOverlayRef = useRef<HTMLDivElement | null>(null);
  const [bottomReserve, setBottomReserve] = useState<number | null>(null);
  // The bound overlay's measure-and-apply, so the per-commit re-measure can reuse
  // it; `null` while no overlay is mounted.
  const remeasureBottomReserveRef = useRef<(() => void) | null>(null);

  // Track the bottom overlay's actual height and drive the body's bottom padding
  // from it (height + the overlay inset as a gap), so resting content clears the
  // composer however tall it grows — the last turn sits just above it rather than
  // being covered by a fixed reserve. When the overlay grows (the composer
  // expands as you type, or a banner appears) the body's padding grows and so
  // does its scrollHeight; re-stick in the same measurement so, while sticking,
  // the tail stays pinned just above the composer.
  //
  // This observes the OVERLAY, never the body, and only ever writes
  // `bottomReserve` (the body's padding) and the body's `scrollTop` — never the
  // overlay's own size — so it cannot feed its own observation back into a loop.
  // The body's ResizeObserver (in `useStickToBottom`) observes the body and
  // only writes `scrollTop`, so the two stay independent. Re-bind when the
  // overlay's presence changes (e.g. a resume-unavailable session drops the
  // composer) so the ref tracks the live node; clear the reserve when there is
  // no overlay so the body falls back to the fixed token.
  //
  // The per-commit re-measure below passes `onlyIfChanged`, so on the renders
  // that leave the overlay's height alone it costs one read and changes nothing.
  useLayoutEffect(() => {
    const overlay = bottomOverlayRef.current;
    if (!overlay) {
      // No bottom overlay (e.g. resume-unavailable drops the composer): clear
      // the measured reserve so the body falls back to the fixed token below.
      setBottomReserve(null);
      return;
    }
    let applied: number | null = null;
    const apply = (onlyIfChanged: boolean) => {
      // While a branch is being composed, the "Branch from selected text" banner
      // adds height to this overlay. Folding that into the body's bottom reserve
      // (and the stick-to-bottom re-scroll) shifts the transcript the instant
      // text is selected — moving the very selection the user is trying to
      // adjust, and making it hard to read. So while a branch is pending, hold
      // the reserve and skip the re-scroll: the banner floats over the transcript
      // tail instead of pushing it. The reserve recomputes once the branch is
      // cleared or sent (this effect re-runs and apply() proceeds normally).
      if (useComposerStore.getState().branchOrigin !== null) {
        return;
      }
      const reserve =
        overlay.getBoundingClientRect().height +
        overlayInsetPx(overlay) +
        BODY_BOTTOM_READING_GAP_PX;
      if (onlyIfChanged && reserve === applied) {
        return;
      }
      applied = reserve;
      setBottomReserve(reserve);
      const body = bodyRef.current;
      if (body && stickRef.current) {
        body.scrollTop = body.scrollHeight;
      }
    };
    apply(false);
    remeasureBottomReserveRef.current = () => apply(true);
    const observer = new ResizeObserver(() => apply(false));
    observer.observe(overlay);
    return () => {
      observer.disconnect();
      remeasureBottomReserveRef.current = null;
    };
  }, [hasBottomOverlay]);

  // Re-measure after every commit of this pane as well. A render that changes
  // what the overlay holds — focusing a closed session adds the read-only notice
  // above the composer in the very commit that swaps the transcript — resizes
  // the overlay at once, but the observer reports it only at the next rendering
  // step, and the reserve it then sets commits in a later task still. In between,
  // a frame is painted (and the layout can be read) with the conversation sized
  // for the old overlay. A state update made here commits synchronously, before
  // the browser gets control back, so the reserve lands with the change that
  // caused it. Growth that happens without this pane re-rendering (the composer
  // auto-growing as you type) is still the observer's to report.
  useLayoutEffect(() => {
    remeasureBottomReserveRef.current?.();
  });

  return { bottomOverlayRef, bottomReserve };
}
