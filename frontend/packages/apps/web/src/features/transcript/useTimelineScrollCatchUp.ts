import { useEffect, useRef, type MutableRefObject } from 'react';
import type { SortedMessage } from './timelineLanes';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';

interface TimelineScrollCatchUpArgs {
  /** Bumped by every route that moves the active message; 0 means none yet. */
  userActedTick: number;
  /** The horizontal-scroll container of the lane grid. */
  axisScrollRef: MutableRefObject<HTMLDivElement | null>;
  /**
   * The horizontal-scroll container's measured width (px); 0 while it is
   * unmounted or not measured yet. A change between two non-zero widths
   * re-runs the horizontal visibility check without a navigation.
   */
  axisViewportWidth: number;
  /** The vertically scrolling body wrapper around the lane grid. */
  bodyRef: MutableRefObject<HTMLDivElement | null>;
  /** The message the playhead sits on, or null. */
  activeMessage: SortedMessage | null;
  /** The shared lane axis width (px). */
  laneAxisWidth: number;
  /** Each message's x (px) on the shared lane axis. */
  messagePxByUuid: Map<string, number>;
  /**
   * Identifies what the timeline is showing (the focused session id). A
   * change while the timeline stays mounted and expanded counts as a new
   * showing, so the new session's playhead is placed like a fresh expand.
   */
  showingKey: string | null;
  /**
   * True once every lane's messages have finished their first fetch. The
   * initial placement waits for it: lanes that land later re-lay the shared
   * x map, which would move the playhead off a placement made too early.
   */
  lanesSettled: boolean;
}

/**
 * Scroll the lane grid horizontally so the playhead sits inside the visible
 * axis band, the part of the viewport to the right of the sticky label
 * column. Does nothing while the playhead is already inside that band with a
 * comfortable margin on both sides.
 *
 * The scroll container hosts BOTH the sticky label column and the axis cells
 * (one grid, `grid-template-columns: max-content 1fr`), so scroll-content
 * x=0 sits at the label cell's left edge. A mark's content-space x is
 * therefore `labelWidth + LANE_LEFT_PAD_PX + xInAxis`; the label width is
 * read live from the first axis cell's `offsetLeft`, the same for every
 * lane because all lanes share one grid column.
 *
 * The sticky label (`position: sticky; left: 0; zIndex: 1`, opaque) paints
 * over viewport-x `[0, labelWidth]`, so the playhead is hidden as soon as it
 * enters that band. The visible axis band is `[labelWidth, clientWidth]` in
 * viewport space, and both the trigger and the landing are expressed against
 * it:
 *
 *  - Trigger: the left threshold is `viewLeft + labelWidth + margin`, the
 *    right one `viewRight - margin` (nothing covers the right edge). The
 *    margin re-centres the playhead BEFORE it reaches either boundary, so it
 *    never visibly vanishes during a scrub.
 *  - Landing: the playhead lands in the middle of the visible axis band, not
 *    of the whole viewport. Centring on the whole viewport would put the
 *    playhead under the label whenever the label column is wider than half
 *    the viewport.
 */
function scrollPlayheadIntoAxisBand(
  scrollEl: HTMLDivElement,
  playheadXInAxis: number,
  behavior: ScrollBehavior,
): void {
  const axisEl = scrollEl.querySelector<HTMLElement>('[data-timeline-axis]');
  // Falling back to 0 treats the axis as starting at the scroll origin if
  // the query misses (e.g. before first paint).
  const labelOffsetPx = axisEl?.offsetLeft ?? 0;
  const playheadInContent = labelOffsetPx + playheadXInAxis + LANE_LEFT_PAD_PX;
  const viewportWidth = scrollEl.clientWidth;
  // The margin is `max(80, clientWidth / 5)`: 80 px is a hard floor so the
  // threshold is meaningful on narrow panels, and 20% of the viewport scales
  // it up on wider ones.
  const margin = Math.max(80, viewportWidth / 5);
  const viewLeft = scrollEl.scrollLeft;
  const viewRight = viewLeft + viewportWidth;
  const leftEdgeThreshold = viewLeft + labelOffsetPx + margin;
  const rightEdgeThreshold = viewRight - margin;
  if (
    playheadInContent >= leftEdgeThreshold &&
    playheadInContent <= rightEdgeThreshold
  ) {
    return;
  }
  // Middle of the visible axis band, in viewport space. Landing there
  // maximises the distance to either edge before the next step can trigger
  // another scroll.
  const bandCentre = labelOffsetPx + (viewportWidth - labelOffsetPx) / 2;
  scrollEl.scrollTo({
    left: Math.max(0, playheadInContent - bandCentre),
    behavior,
  });
}

/**
 * Place the playhead inside the lane grid's visible axis band when the
 * timeline is shown, keep it there and the active lane row inside its
 * vertical viewport after every user-driven navigation, and bring the
 * playhead back into the band when the timeline is resized.
 */
export function useTimelineScrollCatchUp({
  userActedTick,
  axisScrollRef,
  axisViewportWidth,
  bodyRef,
  activeMessage,
  laneAxisWidth,
  messagePxByUuid,
  showingKey,
  lanesSettled,
}: TimelineScrollCatchUpArgs): void {
  // Initial placement: the playhead auto-anchors on the latest message, at
  // the far right of a long axis, while a freshly mounted scroll container
  // sits at scrollLeft 0. Without this the timeline would open with the
  // playhead thousands of pixels off-screen until the first navigation.
  //
  // A showing starts when the scroll container gets a non-zero width
  // (expanding remounts it; a collapse unmounts it and resets the width to
  // 0) or when `showingKey` changes under a mounted timeline. The lane
  // messages are fetched only after the expand, so the placement waits
  // until the width, the active message, and its x are all known. It runs
  // once per showing: afterwards only the navigation and resize paths below
  // move the scroll, so a background refetch never overrides a position
  // the user picked with the scrollbar.
  //
  // The scroll is instant: a timeline that has just opened should already
  // show the playhead rather than slide the axis across the screen.
  const placedShowingRef = useRef<{ key: string | null } | null>(null);
  useEffect(() => {
    if (axisViewportWidth === 0) {
      placedShowingRef.current = null;
      return;
    }
    if (placedShowingRef.current?.key === showingKey) {
      return;
    }
    if (!lanesSettled || activeMessage === null) {
      return;
    }
    const playheadXInAxis = messagePxByUuid.get(activeMessage.uuid);
    const scrollEl = axisScrollRef.current;
    if (playheadXInAxis === undefined || !scrollEl) {
      return;
    }
    placedShowingRef.current = { key: showingKey };
    scrollPlayheadIntoAxisBand(scrollEl, playheadXInAxis, 'auto');
  }, [
    axisViewportWidth,
    showingKey,
    lanesSettled,
    activeMessage,
    messagePxByUuid,
    axisScrollRef,
  ]);

  // After a user-driven navigation, ensure the playhead is visible inside
  // the visible axis band. The lane axis is fixed-width so this only matters
  // when the viewport is narrower than the label column plus the axis (e.g.
  // a narrow window); when everything fits, no scroll is needed.
  //
  // Gated on {@link userActedTick} (not `scrubTick`) so EVERY route that
  // moves the active message — wheel/click jump, pane-scroll follower, and
  // Navigator-driven cross-pane switch — keeps the playhead visible. Before
  // the first navigation (`userActedTick === 0`) the initial placement above
  // owns the horizontal scroll.
  //
  // Smooth, unlike the initial placement: the re-centre follows a step the
  // user just made, so animating it keeps the move legible. The native API
  // also honours `prefers-reduced-motion` with no explicit branch here.
  useEffect(() => {
    if (userActedTick === 0) {
      return;
    }
    const scrollEl = axisScrollRef.current;
    if (!scrollEl || activeMessage === null) {
      return;
    }
    scrollPlayheadIntoAxisBand(
      scrollEl,
      messagePxByUuid.get(activeMessage.uuid) ?? 0,
      'smooth',
    );
  }, [userActedTick, activeMessage, laneAxisWidth, messagePxByUuid]);

  // Resize path: narrowing the window shrinks the visible axis band (and the
  // capped label column with it), which can leave the playhead under the
  // sticky label or past the right edge with no navigation to trigger the
  // effect above. Re-run the same check whenever the measured width changes
  // between two non-zero values. The first measurement after mount (0 → w)
  // is left to the initial placement above, which also waits for the lane
  // messages. Other dependency changes leave the width unchanged and return
  // early: the navigation path above owns those.
  const previousAxisViewportWidthRef = useRef(axisViewportWidth);
  useEffect(() => {
    const previous = previousAxisViewportWidthRef.current;
    previousAxisViewportWidthRef.current = axisViewportWidth;
    if (
      previous === 0 ||
      axisViewportWidth === 0 ||
      previous === axisViewportWidth
    ) {
      return;
    }
    const scrollEl = axisScrollRef.current;
    if (!scrollEl || activeMessage === null) {
      return;
    }
    scrollPlayheadIntoAxisBand(
      scrollEl,
      messagePxByUuid.get(activeMessage.uuid) ?? 0,
      'smooth',
    );
  }, [axisViewportWidth, axisScrollRef, activeMessage, messagePxByUuid]);

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
  //     scroll container is both simpler and needs no label-offset
  //     correction.
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
