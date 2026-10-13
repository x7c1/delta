import { useEffect, type MutableRefObject } from 'react';
import type { SortedMessage } from './timelineLanes';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';

interface TimelineScrollCatchUpArgs {
  /** Bumped by every route that moves the active message; 0 means none yet. */
  userActedTick: number;
  /** The horizontal-scroll container of the lane grid. */
  axisScrollRef: MutableRefObject<HTMLDivElement | null>;
  /** The vertically scrolling body wrapper around the lane grid. */
  bodyRef: MutableRefObject<HTMLDivElement | null>;
  /** The message the playhead sits on, or null. */
  activeMessage: SortedMessage | null;
  /** The shared lane axis width (px). */
  laneAxisWidth: number;
  /** Each message's x (px) on the shared lane axis. */
  messagePxByUuid: Map<string, number>;
}

/**
 * Keep the playhead inside the lane grid's horizontal viewport and the active
 * lane row inside its vertical one after every user-driven navigation.
 */
export function useTimelineScrollCatchUp({
  userActedTick,
  axisScrollRef,
  bodyRef,
  activeMessage,
  laneAxisWidth,
  messagePxByUuid,
}: TimelineScrollCatchUpArgs): void {
  // After a user-driven navigation, ensure the playhead is visible inside
  // the axis column's horizontal viewport. The lane axis is fixed-width so
  // this only matters when the viewport is narrower than the axis (e.g. a
  // narrow side panel); when the axis fits, no scroll is needed.
  //
  // v31 fix 1: re-centre as soon as the playhead crosses INTO the edge
  // margin, not only after it has gone completely off-screen. Without the
  // margin, the scroll catch-up fires one full edge-width late and the
  // playhead visibly disappears for ~one viewport before the scroll lands.
  //
  // v31 fix 2 (label-width offset): under the v20 grid layout the same
  // scroll container hosts BOTH the sticky label column AND the axis cell
  // — `<ul style="grid-template-columns: max-content 1fr; width: max-content">`
  // — so scroll-content x=0 sits at the label cell's left edge, not at the
  // axis's left edge. The dots' content-space x is therefore
  // `labelWidth + LANE_LEFT_PAD_PX + xInAxis`, where the leading
  // `labelWidth` only enters via the axis cell's `offsetLeft` (the prior
  // v9 layout did not have this offset because labels lived outside the
  // scroll container). The previous math used `LANE_LEFT_PAD_PX +
  // xInAxis` alone, so the visibility check sat `labelWidth` pixels to
  // the left of where the playhead actually paints — early-message
  // playheads read as "in view" while their on-screen position was
  // already past the right edge, and late-message playheads triggered a
  // scroll past the real content.
  //
  // v31 fix 3 (asymmetric edge thresholds): the sticky label cell paints
  // over viewport-x `[0, labelOffsetPx]` on every frame (it is
  // `position: sticky; left: 0; zIndex: 1` and the playhead carries no
  // explicit z-index, so the label wins the stack). That means the
  // playhead becomes physically hidden the moment its viewport-x drops
  // below `labelOffsetPx`, even though it is still inside the scroll
  // viewport mathematically. The left-edge catch-up therefore has to
  // fire AT the back of the sticky band, not at the raw viewport left:
  // `leftEdgeThreshold = viewLeft + labelOffsetPx + margin`. The right
  // edge has no symmetric overlay (nothing pins to the right of the axis
  // column), so it stays `viewRight - margin`. When `labelOffsetPx === 0`
  // the left formula collapses back to `viewLeft + margin`, so this is a
  // pure widening of the trigger band on layouts that do have a label
  // column — the no-label path is unchanged.
  //
  // Gated on {@link userActedTick} (not {@link scrubTick}) so EVERY route
  // that moves the active message — wheel/click jump, pane-scroll follower,
  // and Navigator-driven cross-pane switch — keeps the playhead inside the
  // axis viewport. A fresh mount still sits at `userActedTick === 0` and
  // performs no scroll, matching the prior behaviour. The wider gate is
  // what fixes the dogfooding bug where picking a subthread from the
  // Navigator left the playhead off-screen on long sessions.
  useEffect(() => {
    if (userActedTick === 0) {
      return;
    }
    const scrollEl = axisScrollRef.current;
    if (!scrollEl || activeMessage === null) {
      return;
    }
    // The first axis cell shares its content-space origin with every
    // other lane (one grid column, same `<ul>`), so its `offsetLeft` is
    // the label-column width inside the scroll container. Using the live
    // measurement (rather than a hard-coded width) keeps the fix robust
    // against future label-width tweaks. Falling back to 0 leaves us in
    // the v30 coord system if the query misses (e.g. before first paint).
    const axisEl = scrollEl.querySelector<HTMLElement>('[data-timeline-axis]');
    const labelOffsetPx = axisEl?.offsetLeft ?? 0;
    const playheadInContent =
      labelOffsetPx +
      (messagePxByUuid.get(activeMessage.uuid) ?? 0) +
      LANE_LEFT_PAD_PX;
    // Threshold-based scroll-follow. The margin keeps the playhead inside
    // a comfortable band away from both viewport edges so the bar never
    // visibly vanishes during a scrub: as the user steps the playhead
    // toward an edge, the scroll re-centres BEFORE the bar reaches the
    // boundary. The bound is `max(80, clientWidth / 5)`: 80 px is a hard
    // floor so the threshold is meaningful even on narrow panels, and
    // 20% of the viewport scales the margin up gracefully on wider ones.
    // On a 600 px viewport the margin is 120 px (a fifth); on a 200 px
    // panel it's the 80 px floor. Re-centering puts the playhead at the
    // viewport's midpoint — the same generous landing the off-screen
    // case used in v30 — which maximises the distance to either edge
    // before the next step can trigger another scroll.
    const margin = Math.max(80, scrollEl.clientWidth / 5);
    const viewLeft = scrollEl.scrollLeft;
    const viewRight = viewLeft + scrollEl.clientWidth;
    // Asymmetric thresholds: the left edge accounts for the sticky label
    // overlay (see "v31 fix 3" above) so the catch-up fires before the
    // playhead disappears under it. The right edge stays at the raw
    // viewport boundary because nothing covers it.
    const leftEdgeThreshold = viewLeft + labelOffsetPx + margin;
    const rightEdgeThreshold = viewRight - margin;
    if (
      playheadInContent < leftEdgeThreshold ||
      playheadInContent > rightEdgeThreshold
    ) {
      // Use the native smooth-scroll API so the re-centre animates instead
      // of snapping. A direct `scrollLeft = ...` assignment causes a visible
      // jump as the playhead approaches the viewport edge; `scrollTo` lets
      // the browser interpolate, and it also honours `prefers-reduced-motion`
      // automatically — users who have disabled motion still get an instant
      // jump via the same code path, with no explicit branch on our side.
      scrollEl.scrollTo({
        left: Math.max(0, playheadInContent - scrollEl.clientWidth / 2),
        behavior: 'smooth',
      });
    }
  }, [userActedTick, activeMessage, laneAxisWidth, messagePxByUuid]);

  // Vertical counterpart of the horizontal catch-up above: keep the ACTIVE
  // LANE ROW visible inside the body wrapper's vertical viewport (the
  // `max-h-64 overflow-y-auto` outer wrapper). With enough lanes the lane
  // grid overflows that cap, and a navigation that lands on a lane outside
  // the visible band — a wheel/keyboard step across lanes, a Navigator
  // switch, a pane-scroll follow — would otherwise move the playhead onto a
  // row the user cannot see.
  //
  // Two deliberate asymmetries against the horizontal effect:
  //
  //  1. No edge-margin band. The playhead's x moves continuously through
  //     the horizontal viewport, so the horizontal catch-up needs a margin
  //     to fire BEFORE the bar visually reaches the edge. Vertically the
  //     active lane changes in whole-row steps, and the trigger below
  //     checks the ACTIVE row's own visibility — the moment a navigation
  //     lands on a clipped or hidden row, the effect fires. There is no
  //     intermediate "sliding toward the edge" state for a margin to
  //     anticipate.
  //
  //  2. The geometry comes from `getBoundingClientRect` deltas rather than
  //     content-space math. The horizontal effect has to reconstruct the
  //     playhead's content x from `messagePxByUuid` (the playhead is a
  //     painted offset, not an element with its own scroll geometry); the
  //     lane row IS an element, so measuring rect-vs-rect against the
  //     scroll container is both simpler and immune to the label-offset
  //     coordinate bug the horizontal math had to fix (v31 fix 2).
  //
  // The measured element is the active lane's AXIS CELL, not its `<li>`:
  // the `<li>` is `display: contents` and generates no box, so its rect is
  // empty. The axis cell spans the grid row's full stretched height (see
  // `align-items: stretch` on the lane `<ul>`), so its rect IS the row band
  // the user needs to see.
  useEffect(() => {
    if (userActedTick === 0) {
      return;
    }
    const bodyEl = bodyRef.current;
    if (!bodyEl || activeMessage === null) {
      return;
    }
    const laneEl = bodyEl.querySelector<HTMLElement>(
      '[data-timeline-axis][data-active="true"]',
    );
    if (!laneEl) {
      return;
    }
    const bodyRect = bodyEl.getBoundingClientRect();
    const laneRect = laneEl.getBoundingClientRect();
    // Rect tops are viewport-space; adding the current scrollTop converts
    // the lane's offset-within-viewport into scroll-content space, the
    // same coordinate system `scrollTo({ top })` expects.
    const laneTop = laneRect.top - bodyRect.top + bodyEl.scrollTop;
    const laneBottom = laneTop + laneRect.height;
    const viewTop = bodyEl.scrollTop;
    const viewBottom = viewTop + bodyEl.clientHeight;
    if (laneTop < viewTop || laneBottom > viewBottom) {
      // Centre the row for the same reason the horizontal effect centres
      // the playhead: it maximises the distance to either edge before the
      // next cross-lane step can trigger another scroll. Same smooth-scroll
      // rationale too — the browser animates the catch-up and honours
      // `prefers-reduced-motion` with no explicit branch on our side.
      bodyEl.scrollTo({
        top: Math.max(
          0,
          laneTop + laneRect.height / 2 - bodyEl.clientHeight / 2,
        ),
        behavior: 'smooth',
      });
    }
  }, [userActedTick, activeMessage]);
}
