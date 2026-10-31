import { buildLaneRenderItems, type TimelineLane } from './timelineLanes';
import { TimelineClusterMark, TimelineDotMark } from './TimelineMarks';

/**
 * CSS transition duration (ms) for the playhead's `left` animation. Short
 * enough that the user always feels the playhead is "tracking" their input,
 * long enough that the discrete step between adjacent messages does not
 * teleport jarringly.
 */
const PLAYHEAD_TRANSITION_MS = 100;

/** Lane row height in pixels. */
const LANE_HEIGHT_PX = 18;

/** Width reserved for the right-hand padding inside the lane area. */
const LANE_RIGHT_PAD_PX = 16;

/**
 * Width reserved on the LEFT inside the lane axis so the leftmost mark — a
 * large 6 px dot centred on x=0 — is not clipped by the axis container's
 * `overflow-x-auto` left edge. Without this padding the left half of the
 * earliest dot disappears into the column boundary.
 *
 * Sized to mirror {@link LANE_RIGHT_PAD_PX} so the axis whitespace reads as
 * symmetric on both ends. The dots' `xPx` values come from the shared global
 * map (which still uses 0 as the leftmost time-axis x); the renderer adds
 * this offset to every absolute-positioned child (dot, cluster, playhead,
 * axis line) and the click handler subtracts it before resolving the
 * nearest message. The total rendered axis container width is therefore
 * {@link LANE_LEFT_PAD_PX} + `laneAxisWidth` + {@link LANE_RIGHT_PAD_PX}.
 */
export const LANE_LEFT_PAD_PX = 16;

interface TimelineLaneRowProps {
  /** The lane this row renders: its label cell and its axis cell. */
  lane: TimelineLane;
  /** Whether this lane carries the active-lane highlight. */
  isHighlighted: boolean;
  /** The shared lane axis width (px) every lane's axis cell spans. */
  laneAxisWidth: number;
  /** Each message's x (px) on the shared lane axis. */
  messagePxByUuid: Map<string, number>;
  /** The playhead's x (px) on the shared lane axis. */
  playheadX: number;
  /**
   * The widest the label cell may grow (px), shared by every lane so the
   * label column — and with it every lane's playhead segment — stays
   * aligned. Undefined leaves the label uncapped (before the first
   * measurement).
   */
  labelMaxWidthPx: number | undefined;
}

/**
 * One lane of the timeline grid: the sticky label cell and the axis cell with
 * the lane's marks and its segment of the playhead.
 */
export function TimelineLaneRow({
  lane,
  isHighlighted,
  laneAxisWidth,
  messagePxByUuid,
  playheadX,
  labelMaxWidthPx,
}: TimelineLaneRowProps) {
  // The active-lane highlight (border-y + bg-surface-elevated) is
  // applied identically to BOTH cells of the active lane so
  // the visual band spans the full grid row. With
  // `display: contents` on the `<li>` the element itself
  // is stripped from layout (no box is generated), so the
  // `<li>` cannot carry the highlight; each child cell
  // does its own painting and the two halves line up
  // because they share the same grid row.
  //
  // The label cell is sticky-positioned (left:0) and slides
  // over the axis cell as the wrapper pans horizontally, so
  // it MUST be opaque in EVERY state or axis dots would
  // show through the label during a pan. The axis cell can
  // stay transparent in the inactive state (the body's
  // white background reads through it just fine), so the
  // resting background only needs to be added to the label
  // class set. Doing it via className — `bg-surface` resting,
  // `bg-surface-elevated` active — keeps className as the single
  // source of truth for the cell's visual state: an active
  // sticky label paints `bg-surface-elevated` (matching the active
  // axis cell so the band reads as one row), and an
  // inactive sticky label paints `bg-surface` (matching the
  // body so axis dots cannot peek through).
  // The active-row visual treatment is a thin slate-200
  // hairline above and below the row. We render that hairline
  // as a pair of `inset box-shadow`s rather than `border-y`,
  // because `border-y border-transparent` (the previous
  // inactive placeholder used to keep active/inactive heights
  // equal) still reserves 1 px of layout on the top and 1 px
  // on the bottom. With `align-items: stretch` on the lane
  // grid, that placeholder showed up as a ~2 px transparent
  // stripe between adjacent rows — visible as a faint break
  // in the per-lane playhead column, even though all the
  // `<ul>`'s `gap-y-*` had already been removed in v28.
  // `box-shadow` is non-layout, so the active hairline can
  // paint without forcing every other row to reserve the
  // same vertical space, and adjacent rows now sit truly
  // edge-to-edge.
  // Hairline color is sourced from the semantic
  // `--delta-color-border` token so it follows the active
  // theme (replaces the previously hardcoded slate-200
  // literal that happened to match the light value exactly).
  // The arbitrary-value shadow has no slash-opacity, so the
  // bare `rgb(var(...))` is enough — no `<alpha-value>`
  // placeholder needed here.
  const ACTIVE_HAIRLINE_SHADOW =
    'shadow-[inset_0_1px_0_0_rgb(var(--delta-color-border)),inset_0_-1px_0_0_rgb(var(--delta-color-border))]';
  const highlightClasses = isHighlighted
    ? `${ACTIVE_HAIRLINE_SHADOW} bg-surface-elevated`
    : '';
  const labelHighlightClasses = isHighlighted
    ? `${ACTIVE_HAIRLINE_SHADOW} bg-surface-elevated`
    : 'bg-surface';
  // Collapse runs of 2+ consecutive small dots within
  // this lane into one cluster mark so a long stretch of
  // tool calls / meta lines no longer floods the
  // timeline. Lone small dots and every large dot still
  // render individually.
  const renderItems = buildLaneRenderItems(lane.dots);
  return (
    // `display: contents` keeps the <li> for semantics /
    // a11y (the list still has list items) while removing
    // its box from layout so the inner label and axis
    // become direct grid items of the <ul>.
    <li
      data-testid="thread-timeline-lane"
      data-thread-id={lane.threadId}
      data-active={isHighlighted ? 'true' : 'false'}
      style={{ display: 'contents' }}
    >
      <span
        title={lane.tooltip}
        data-testid="thread-timeline-lane-label"
        data-thread-id={lane.threadId}
        data-active={isHighlighted ? 'true' : 'false'}
        // `position: sticky; left: 0` pins the label cell
        // to the left edge of the horizontal-scroll
        // wrapper as the axis pans, so labels stay
        // readable while the user scrubs a wide session.
        // The opaque background (from `labelHighlightClasses`
        // — `bg-surface` resting, `bg-surface-elevated` active)
        // prevents axis dots from peeking through during
        // the pan; the z-index keeps the label above the
        // axis line and dots. The background lives on the
        // className (not inline) so the active highlight's
        // `bg-surface-elevated` is the one that paints — an inline
        // background would win over the class and would
        // leave the sticky label white in the active state,
        // breaking the visual continuity with the axis
        // cell's highlight.
        // `h-full` opts the sticky label into the grid row's
        // full stretched height (see the `align-items:
        // stretch` rationale on the lane `<ul>`), so the
        // label-side background band paints the same vertical
        // extent as the axis-side band. `display: flex` +
        // `alignItems: center` keeps the glyph vertically
        // centred when the cell is taller than the text —
        // the old `lineHeight: LANE_HEIGHT_PX` centred only
        // at exactly that height and would leave the glyph
        // pinned to the top of a taller stretched cell.
        // `maxWidth` caps the label so a long subthread name
        // cannot widen the sticky column over most of the
        // axis. `text-overflow` does not apply to the text of
        // a flex container, so the ellipsis lives on the inner
        // span (a flex item that may shrink below its content
        // width because it clips its overflow); the full name
        // stays in the `title` tooltip.
        className={`flex h-full items-center overflow-hidden whitespace-nowrap rounded-xs py-0.5 pl-1 pr-2 font-mono text-code ${
          lane.isMain ? 'text-fg' : 'text-fg-muted'
        } ${labelHighlightClasses}`}
        style={{
          position: 'sticky',
          left: 0,
          zIndex: 1,
          minHeight: LANE_HEIGHT_PX,
          maxWidth: labelMaxWidthPx,
        }}
      >
        <span className="min-w-0 truncate">{lane.label}</span>
      </span>
      <div
        data-timeline-axis=""
        data-thread-id={lane.threadId}
        data-active={isHighlighted ? 'true' : 'false'}
        // `h-full` lets the axis cell expand to the grid
        // row's stretched height (see `align-items: stretch`
        // on the lane `<ul>`) so the axis-side highlight
        // band lines up with the label-side band. The
        // explicit `LANE_HEIGHT_PX` becomes a `minHeight`
        // floor (no-content rows still respect the lane
        // height) rather than a fixed `height` cap that
        // would defeat the stretch.
        className={`relative h-full rounded-xs ${highlightClasses}`}
        style={{
          width:
            LANE_LEFT_PAD_PX +
            laneAxisWidth +
            LANE_RIGHT_PAD_PX,
          minHeight: LANE_HEIGHT_PX,
        }}
      >
        <span
          aria-hidden="true"
          className="absolute top-1/2 h-px -translate-y-1/2 bg-border-default"
          style={{
            left: LANE_LEFT_PAD_PX,
            width: laneAxisWidth,
          }}
        />
        {renderItems.map((item) =>
          item.kind === 'dot' ? (
            <TimelineDotMark
              key={item.dot.uuid}
              dot={item.dot}
              xPx={
                (messagePxByUuid.get(item.dot.uuid) ?? 0) +
                LANE_LEFT_PAD_PX
              }
            />
          ) : (
            <TimelineClusterMark
              key={item.cluster.key}
              cluster={item.cluster}
              xPx={
                (messagePxByUuid.get(
                  item.cluster.representativeUuid,
                ) ?? 0) + LANE_LEFT_PAD_PX
              }
            />
          ),
        )}
        {/* Playhead: a thin vertical line that doubles as
            the lane-local segment of the global playhead.
            Each lane carries its own copy (instead of one
            absolute line spanning the column) so it
            scrolls with the axis when the column becomes
            horizontally scrollable on a long session. A
            short CSS transition on `left` smooths the
            discrete step between adjacent messages so the
            eye can follow the move; the duration is short
            enough that the playhead always feels glued to
            the input. */}
        <span
          aria-hidden="true"
          data-testid="thread-timeline-playhead"
          // `left-0` plus `transform: translateX(...)` instead
          // of inline `left: <fractional px>`: at width 2 px,
          // a fractional `left` lets the browser straddle a
          // subpixel boundary so antialiasing paints ~1.5 px
          // on one side and ~0.5 px on the other — the bar
          // visibly shimmers between fat and thin as the
          // playhead steps across messages. `translateX` is
          // GPU-composited on the existing 2 px box so the
          // sprite keeps a stable 2 px footprint regardless
          // of where it lands on the subpixel grid.
          className="pointer-events-none absolute left-0 top-0 h-full w-px bg-fg-muted"
          style={{
            transform: `translateX(${playheadX + LANE_LEFT_PAD_PX}px)`,
            transition: `transform ${PLAYHEAD_TRANSITION_MS}ms ease-out`,
          }}
        />
      </div>
    </li>
  );
}
