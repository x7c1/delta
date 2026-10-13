import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from 'react';
import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useThreadsMessagesQueries } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import {
  buildGlobalXMap,
  buildLargeSortedMessages,
  buildSortedMessages,
  buildTimelineLanes,
  computeTimeRange,
  findNearestMessageIndex,
} from './timelineLanes';
import {
  SkipBackIcon,
  SkipForwardIcon,
  ThreadTimelineIcon,
} from './TimelineIcons';
import { markDiameterPx } from './TimelineMarks';
import { useTimelineExpanded } from './useTimelineExpanded';
import {
  useTimelineKeyboardStepNavigation,
  useTimelineWheelStepNavigation,
} from './useTimelineStepNavigation';
import { useTimelinePlayhead } from './useTimelinePlayhead';
import { useTimelinePaneJump } from './useTimelinePaneJump';
import { LANE_LEFT_PAD_PX, TimelineLaneRow } from './TimelineLaneRow';
import { useTimelineScrollCatchUp } from './useTimelineScrollCatchUp';
import { useTimelinePaneScrollFollow } from './useTimelinePaneScrollFollow';

export interface ThreadTimelineOverlayProps {
  /** All threads (main + subthreads) in the focused session. */
  threads: Thread[];
  /** The active thread; lane highlight defaults here until the playhead lands. */
  activeThreadId: ThreadId | null;
  /**
   * The conversation-pane scroll container the playhead's active-message jump
   * targets. The lookup is scoped to it so an off-screen duplicate id (e.g.
   * a portaled preview) does not misdirect the scroll.
   */
  conversationBodyRef: RefObject<HTMLElement | null>;
}

/** Minimum width (in px) the lane axis reserves so a single-dot session is still scrubbable. */
const MIN_LANE_AXIS_PX = 240;

/**
 * Tailwind class string for the collapsed-state toggle button. Mirrors the
 * pane-toggle button's shape exactly so the two buttons sit side-by-side in the
 * top region without one reading as a different control type. The pane-toggle
 * button (Terminal or Comms, depending on the focused provider) lives in
 * `WorkspaceScreen` and uses the same class chain — kept in sync via
 * `PANE_TOGGLE_BUTTON_CLASS` over there.
 */
export const TIMELINE_TOGGLE_BUTTON_CLASS =
  'inline-flex items-center gap-1.5 rounded-md border border-border-default bg-surface px-3 py-1.5 text-caption font-medium text-fg shadow-md transition-colors hover:bg-surface-elevated';

/**
 * Tailwind class string for the expanded-state jump-to-edge buttons (skip-back
 * / skip-forward) that sit on the LEFT of the expanded header row.
 *
 * Visually intentionally lighter than the Timeline toggle on the right: no
 * border, no fill, no shadow — just the icon in a muted slate, with a soft
 * background tint on hover for click affordance. The icon itself matches the
 * Timeline glyph's `h-3.5 w-3.5` size so the three header icons read as one
 * set; the chrome around it is just what hover/disabled states need and
 * nothing more, so the jump controls do not compete with the toggle pill
 * (which is the primary control in the row).
 *
 * The `disabled:` variants neutralise the buttons when `sortedMessages` is
 * empty (no messages to jump to); we keep them enabled when the playhead
 * already sits at the edge, because re-clicking still bumps `scrubTick` and
 * refreshes the horizontal scroll catch-up, which is a useful "snap me back"
 * affordance.
 */
export const TIMELINE_JUMP_BUTTON_CLASS =
  'inline-flex items-center justify-center rounded p-1 text-fg-subtle transition-colors hover:bg-surface-elevated-hover hover:text-fg disabled:opacity-50 disabled:cursor-not-allowed';

/**
 * The fixed footer between the conversation pane and the composer: a swim-lane
 * timeline of every subthread (and the main thread). Each thread is a row,
 * each speech turn is a mark, and every mark sits on a SHARED time axis driven
 * by the message's `created_at` — idle and thinking gaps render as horizontal
 * whitespace, so the time order is visible at a glance rather than being
 * flattened into equal speech-order spacing.
 *
 * The vertical playhead is a follower: it tracks the active message's x and
 * is never freely draggable. Wheel events advance discretely through the
 * MAIN-CONVERSATION subset of the cross-lane timeline (user turns + Claude's
 * prose replies) so one notch jumps to the next-or-previous headline turn,
 * skipping the surrounding tool calls, meta lines, and question cards. A
 * velocity accelerator scales the step count from the cumulative |delta|
 * inside a short rolling window: one leisurely notch advances one step, a
 * vigorous spin within the window trips higher staircase buckets so a long
 * session can be traversed in a handful of turns instead of dozens. Per-event
 * |delta| is clamped before accumulating so a trackpad's inertial burst
 * stays under control. While expanded, ArrowLeft / ArrowRight step the same
 * large-message subset one message per keydown (left = older, right = newer)
 * with none of the wheel's cooldown or staircase machinery — the
 * deterministic alternative when trackpad inertia makes precise wheel
 * stops unreliable. Clicking anywhere on the timeline jumps the active index
 * to the message whose x is closest to the click — small auxiliary marks are
 * directly tappable, so the user can still reach a specific tool call when
 * they want it. Whichever message is active becomes the lane highlight,
 * fires the existing nav setter, after the next paint is scrolled into view
 * in the conversation pane, and the destination message briefly flashes so
 * the eye catches where the jump landed. Hovering a mark does nothing — the
 * playhead is the single source of truth.
 *
 * The footer is always present; clicking the title bar collapses or expands
 * the lanes, and the preference is persisted per device.
 *
 * Marks are color-coded by author (user vs everything else) and sized by
 * role (large = main conversation, small = auxiliary). Cross-row derivation
 * lines, zoom, and finer-grained "other" coloring (assistant vs tool vs
 * meta) are tracked as separate follow-ups.
 */
export function ThreadTimelineOverlay({
  threads,
  activeThreadId,
  conversationBodyRef,
}: ThreadTimelineOverlayProps) {
  const client = useApiClient();
  // The expanded preference is per-session — read the focused session id from
  // `navStore` rather than threading it through props, so the call site in
  // `TranscriptPane` does not need to know about the storage shape. The
  // new-session sentinel is collapsed to `null`: it is not a real session id
  // and must not write its own localStorage entry (which the GC could not
  // distinguish from a real orphan).
  const focusedSessionId = useNavStore((state) =>
    state.focusedSessionId === NEW_SESSION_FOCUS ? null : state.focusedSessionId,
  );
  const [expanded, toggle] = useTimelineExpanded(focusedSessionId);

  // N+1 is acceptable for MVP; the dedicated `all_threads=true` REST is
  // intentionally deferred. The query keys are shared with the focused
  // thread's `useThreadMessagesQuery`, so its messages are reused — no double
  // request. The fan-out is gated on `expanded` so a collapsed timeline does
  // not race the focused thread's load for the browser's six-per-host HTTP/1.1
  // connection pool at cold start; the focused query keeps fetching through
  // its own enabled gate and its result populates the same cache key, so the
  // timeline reuses it the moment the user expands.
  const threadIds = useMemo(() => threads.map((t) => t.id), [threads]);
  const messagesQueries = useThreadsMessagesQueries(client, threadIds, {
    enabled: expanded,
  });
  const messagesByThread = useMemo(() => {
    const map = new Map<ThreadId, Message[]>();
    for (const entry of messagesQueries) {
      const data = entry.result.data;
      if (data) {
        map.set(entry.threadId, data.messages);
      }
    }
    return map;
  }, [messagesQueries]);

  const lanes = useMemo(
    () => buildTimelineLanes(threads, messagesByThread),
    [threads, messagesByThread],
  );

  // Two parallel sorted lists drive navigation:
  //
  //  - `sortedMessages`: every mark in (created_at asc, seq asc) order. The
  //    click-to-jump handler picks the nearest mark from here, so a small
  //    auxiliary mark (tool call, meta line) is still directly tappable.
  //  - `largeSortedMessages`: only the main-conversation turns (user + Claude
  //    prose). The wheel handler steps through this list so one notch
  //    advances by one headline turn, skipping the surrounding chatter.
  //
  // Keeping the two lists in lockstep means the active index for clicks lives
  // in the global list, while a wheel step finds the next/previous large
  // message and snaps the global index to it.
  const sortedMessages = useMemo(() => buildSortedMessages(lanes), [lanes]);
  const largeSortedMessages = useMemo(
    () => buildLargeSortedMessages(lanes),
    [lanes],
  );

  // Mirror the sorted list into a ref so effects that must read the latest
  // messages WITHOUT re-firing on a background-refetch array-identity change
  // (the navigation jump effect, the wheel/arrow handlers, the cross-lane
  // timeout fallback) can reach it. Declared here — above the first consumer —
  // so those closures never reference it before initialization.
  //
  // Synced in the LAYOUT phase, not the passive one: the wheel / keyboard
  // handlers are native listeners that can fire the instant the marks are on
  // screen, and passive effects are deferred to a later task whenever a commit
  // overruns the scheduler's frame budget. A passive sync would leave those
  // handlers reading the pre-commit list while the user is already looking at
  // (and scrubbing over) the new one. Layout effects run inside the commit, so
  // the mirror is never behind the DOM it describes.
  const sortedMessagesRef = useRef(sortedMessages);
  useLayoutEffect(() => {
    sortedMessagesRef.current = sortedMessages;
  }, [sortedMessages]);

  // One x map shared across every lane. Each message's px x is derived from
  // its `(timeMs, seq)` once, globally, so a message at a given timestamp
  // lands at exactly the same x in every lane — cross-lane jumps line up
  // with the marks the user sees, and the playhead's x always agrees with
  // the dot under it. The map also enforces the minimum spacing that keeps
  // dense clusters readable without any alpha/ring workaround, expanding
  // the axis past `MIN_LANE_AXIS_PX` when overlapping ideal positions get
  // pushed right.
  const timeRange = useMemo(
    () => computeTimeRange(messagesByThread),
    [messagesByThread],
  );
  const { pxByUuid: messagePxByUuid, axisWidth: laneAxisWidth } = useMemo(
    () =>
      buildGlobalXMap(
        sortedMessages,
        timeRange,
        MIN_LANE_AXIS_PX,
        markDiameterPx,
        MIN_LANE_AXIS_PX,
      ),
    [sortedMessages, timeRange],
  );

  const {
    activeMessageIndex,
    scrubTick,
    userActedTick,
    bumpUserActedTick,
    setActiveMessageIndex,
    setActiveMessageIndexFromPaneScroll,
    activeMessage,
    activeMessageRef,
  } = useTimelinePlayhead({
    sortedMessages,
    largeSortedMessages,
    activeThreadId,
    sortedMessagesRef,
  });

  // The playhead's x in pixels along the shared lane axis. Resolved through
  // the global map so the playhead and the mark under it always agree, even
  // when overlap mitigation pushed the mark off its ideal time-axis x.
  const playheadX =
    activeMessage === null ? 0 : messagePxByUuid.get(activeMessage.uuid) ?? 0;

  const { crossLaneJumpInFlightCountRef, lastProgrammaticScrollAtRef } =
    useTimelinePaneJump({
      activeThreadId,
      conversationBodyRef,
      sortedMessages,
      largeSortedMessages,
      sortedMessagesRef,
      scrubTick,
      activeMessageRef,
      setActiveMessageIndexFromPaneScroll,
      bumpUserActedTick,
    });

  // The lane the highlight follows: the active message's lane when there is
  // one, else fall back to the prop so a freshly-mounted footer still marks
  // the active thread before any messages have loaded.
  const highlightedThreadId = activeMessage?.threadId ?? activeThreadId;

  // The scrubbable area is the axis column alone: a wheel over the labels
  // column should behave like a normal vertical page scroll, so the wheel
  // listener attaches to the axis container, not the outer body. The axis
  // container's onWheel is the entry point — its `passive: false` listener
  // (registered via the effect below) is required to call `preventDefault`
  // and suppress the page scroll while scrubbing.
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const axisScrollRef = useRef<HTMLDivElement | null>(null);
  // Both mirrors feed the native wheel / keyboard handlers, so they follow the
  // same layout-phase discipline as `sortedMessagesRef` above: a step must be
  // computed from the playhead position and the mark list the user can
  // actually see, never from the commit before it.
  const activeMessageIndexRef = useRef(activeMessageIndex);
  useLayoutEffect(() => {
    activeMessageIndexRef.current = activeMessageIndex;
  }, [activeMessageIndex]);
  const largeSortedMessagesRef = useRef(largeSortedMessages);
  useLayoutEffect(() => {
    largeSortedMessagesRef.current = largeSortedMessages;
  }, [largeSortedMessages]);

  // Wheel scrubbing and ArrowLeft / ArrowRight stepping share the same
  // large-message step semantics. Both handlers live in
  // `useTimelineStepNavigation.ts` and commit through
  // {@link setActiveMessageIndex}, reading the latest sorted lists through
  // the refs kept in sync above.
  useTimelineWheelStepNavigation({
    expanded,
    axisScrollRef,
    sortedMessagesRef,
    largeSortedMessagesRef,
    activeMessageIndexRef,
    setActiveMessageIndex,
  });
  useTimelineKeyboardStepNavigation({
    expanded,
    sortedMessagesRef,
    largeSortedMessagesRef,
    activeMessageIndexRef,
    setActiveMessageIndex,
  });

  // Click anywhere on an axis cell jumps the active index to the message
  // whose x is closest to the click. Clicks on a label cell are ignored —
  // the click target's closest `[data-timeline-axis]` ancestor must exist,
  // otherwise the click lives in the label area (or in a non-cell gap) and
  // the playhead is left alone. There is no distance threshold inside the
  // axis: a click anywhere in it accepts the global nearest message — the
  // overlay is small enough that the closest mark is always the user's
  // intent, and a threshold would otherwise swallow clicks on the empty
  // axis whitespace between marks.
  const handleClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      const scrollEl = axisScrollRef.current;
      if (!scrollEl) {
        return;
      }
      const target = event.target as Element | null;
      if (target && target.closest('[data-testid="thread-timeline-lane-label"]')) {
        return;
      }
      // Locate the first lane axis row (every lane shares the same axis
      // width and x-origin) so the click → x conversion uses the axis the
      // dots actually sit on. Falling back to the scroll container's rect
      // would include the right-hand padding, throwing the conversion off.
      const axisEl = scrollEl.querySelector<HTMLElement>('[data-timeline-axis]');
      if (!axisEl) {
        return;
      }
      const rect = axisEl.getBoundingClientRect();
      if (laneAxisWidth <= 0) {
        return;
      }
      // Translate the click to the same absolute-px space the global x map
      // uses. The axis rect now includes {@link LANE_LEFT_PAD_PX} on the
      // left (so the leftmost large dot is not clipped), so subtract it
      // before resolving the nearest message. Clamp to [0, axisWidth] so a
      // click in either pad still snaps to the nearest mark.
      const offsetPx = event.clientX - rect.left - LANE_LEFT_PAD_PX;
      const clampedPx = Math.max(0, Math.min(laneAxisWidth, offsetPx));
      const nearest = findNearestMessageIndex(
        sortedMessages,
        messagePxByUuid,
        clampedPx,
      );
      if (nearest < 0) {
        return;
      }
      setActiveMessageIndex(nearest);
    },
    [laneAxisWidth, sortedMessages, messagePxByUuid, setActiveMessageIndex],
  );

  useTimelineScrollCatchUp({
    userActedTick,
    axisScrollRef,
    bodyRef,
    activeMessage,
    laneAxisWidth,
    messagePxByUuid,
  });

  useTimelinePaneScrollFollow({
    expanded,
    conversationBodyRef,
    sortedMessages,
    setActiveMessageIndexFromPaneScroll,
    activeThreadId,
    crossLaneJumpInFlightCountRef,
    lastProgrammaticScrollAtRef,
  });

  // The collapsed state is just a single button styled to match the
  // pane-toggle button that sits alongside it in the top region (see
  // {@link TIMELINE_TOGGLE_BUTTON_CLASS}). The expanded state grows
  // downward into a card whose chrome (border / shadow / background)
  // matches the breadcrumb and composer cards (the shared
  // {@link FLOATING_CARD_CLASS} family in `floatingCard.ts`). Keeping the
  // collapsed toggle visually identical to the pane-toggle button — and
  // the expanded card visually identical to the other delta UI cards —
  // is what makes the timeline land in the top region without reading
  // as a third style of chrome.
  if (!expanded) {
    return (
      <button
        type="button"
        onClick={toggle}
        data-testid="thread-timeline-toggle"
        data-expanded="false"
        aria-expanded={false}
        aria-label="Timeline"
        className={TIMELINE_TOGGLE_BUTTON_CLASS}
      >
        <ThreadTimelineIcon className="h-3.5 w-3.5" />
        Timeline
      </button>
    );
  }
  // Disable the jump buttons when there is nothing to jump to. The
  // single-source flag matches `setActiveMessageIndex`'s own empty-list guard
  // (L927) — calling it with no messages is a no-op anyway, but visibly
  // dimming the buttons reads as "no targets" rather than "broken control".
  // Clicking either button at the edge (e.g. "jump to start" when already at
  // index 0) is intentionally still allowed — it bumps `scrubTick` which
  // re-fires the horizontal-scroll catch-up, a useful "snap me back" gesture.
  const noMessages = sortedMessages.length === 0;
  return (
    <section
      data-testid="thread-timeline-overlay"
      data-expanded="true"
      className="select-none rounded-md border border-border-default bg-surface text-caption text-fg-muted shadow-md"
      aria-label="Subthread timeline"
    >
      {/* The expanded header is a two-region row:
          LEFT  — the expand/collapse toggle (icon + "Timeline" label).
                  The toggle is the primary control and sits in the first
                  child position so the eye lands on it first when the
                  header row enters view. No visible chevron — the
                  button's `hover:bg-surface-elevated` carries the click
                  affordance, and `aria-expanded` carries the
                  open/collapsed state semantically for assistive tech.
                  The toggle's own `px-3` provides the only left inset
                  (12 px), matching the original single-pill layout.
          RIGHT — two jump-to-edge buttons ([⏮][⏭]) for one-shot
                  navigation to the very first / very last message across
                  ALL lanes. They live in their own flex wrapper with no
                  gap class so the two buttons sit flush against each
                  other — they read as one "jump cluster", a quieter
                  satellite to the toggle pill rather than two
                  independent controls. The icons render at 12 px
                  (`h-3 w-3`) — one notch smaller than the toggle's
                  `h-3.5 w-3.5` — for the same "quieter satellite"
                  reason. The jump buttons live outside the toggle so a
                  click on either one does NOT collapse the timeline.
                  The wrapper's `pr-3` keeps the right inset symmetric
                  with the left (12 px before the rightmost jump
                  button). */}
      <div className="flex w-full items-center justify-between pr-1">
        <button
          type="button"
          onClick={toggle}
          data-testid="thread-timeline-toggle"
          aria-expanded={expanded}
          className="flex flex-1 items-center gap-1.5 rounded-md px-3 py-1.5 text-caption font-medium text-fg transition-colors hover:bg-surface-elevated"
        >
          <span aria-hidden="true" className="text-fg-subtle">▾</span>
          Timeline
        </button>
        <div className="flex items-center">
          <button
            type="button"
            onClick={() => setActiveMessageIndex(0)}
            disabled={noMessages}
            data-testid="thread-timeline-jump-start"
            aria-label="Jump to timeline start"
            className={TIMELINE_JUMP_BUTTON_CLASS}
          >
            <SkipBackIcon className="h-3 w-3" />
          </button>
          <button
            type="button"
            onClick={() => setActiveMessageIndex(sortedMessages.length - 1)}
            disabled={noMessages}
            data-testid="thread-timeline-jump-end"
            aria-label="Jump to timeline end"
            className={TIMELINE_JUMP_BUTTON_CLASS}
          >
            <SkipForwardIcon className="h-3 w-3" />
          </button>
        </div>
      </div>
      {expanded && (
        <div
          ref={bodyRef}
          data-testid="thread-timeline-body"
          // Outer wrapper: vertical scroll only. Horizontal scroll lives on
          // the axis-column wrapper below so the sticky label cells can pin
          // to the left edge as the user pans a wide axis.
          className="max-h-64 overflow-y-auto px-2 pb-1"
        >
          {lanes.length === 0 ? (
            <p className="px-1 py-1 text-caption text-fg-subtle">
              No threads to show yet.
            </p>
          ) : (
            <div
              ref={axisScrollRef}
              data-testid="thread-timeline-axis-column"
              // The single horizontal-scroll container for the whole lane
              // grid. The wheel listener attaches here (it discriminates
              // by event-target so a wheel over a label cell does not
              // scrub) and so does the click handler (same discrimination
              // — a click on a label cell is ignored). `scrollbar-none`
              // hides the bar in both WebKit and Firefox while keeping
              // wheel / trackpad scroll behaviour intact.
              className="scrollbar-none overflow-x-auto"
              onClick={handleClick}
            >
              {/* CSS Grid is the layout primitive that solves both visual
                  issues at once. `grid-template-columns: max-content 1fr`
                  sizes the label column to the widest label across every
                  lane, so all lane labels share the same width and there is
                  no hard-coded fixed gutter. `align-items: center` centres
                  each row's two cells on the same baseline, eliminating
                  the cumulative drift the prior flex layout suffered when
                  the label cell's padding inflated its height past the
                  axis cell's fixed pixel height. The lane rows share
                  `row-gap: 0` (the Tailwind default — no `gap-y-*` class)
                  so the per-lane playhead spans align edge-to-edge across
                  rows; any non-zero row gap would show as a visible break
                  in the otherwise continuous vertical playhead line. */}
              <ul
                data-testid="thread-timeline-lane-grid"
                role="list"
                // `width: max-content` and `minWidth: 100%` together stretch
                // the grid container to the natural width of its widest row
                // — the axis cell carries an explicit pixel width
                // (LANE_LEFT_PAD_PX + laneAxisWidth + LANE_RIGHT_PAD_PX), so
                // `max-content` resolves to that full scrollable range,
                // while `minWidth: 100%` keeps the `<ul>` at least viewport
                // wide on short sessions where the axis fits without
                // scroll. This matters because the label cell uses
                // `position: sticky; left: 0`, and sticky only moves
                // within its containing block (this `<ul>`). Without the
                // width hint the block-level `<ul>` would stay at the
                // visible viewport width even while its axis-cell child
                // overflows the horizontal-scroll wrapper above — and
                // `left: 0` would have nowhere to slide, so the label
                // would scroll off-screen with the axis. Stretching the
                // containing block to the full scroll range gives sticky
                // somewhere to pin against.
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'max-content 1fr',
                  // `align-items: stretch` makes each row's two grid items
                  // (the sticky label cell and the axis cell) grow to the
                  // tallest of the pair. Under `center` the axis cell — which
                  // declares an explicit `LANE_HEIGHT_PX` height — could
                  // render shorter than the label cell whose intrinsic
                  // height is governed by font metrics + padding, producing
                  // a vertically mismatched active-highlight band (the
                  // axis-side bg-surface-elevated painted a thinner stripe than the
                  // label-side stripe) and a per-lane playhead that looked
                  // disconnected between rows because its `h-full` only
                  // filled the shorter axis cell. Stretching guarantees the
                  // two cells share the row's full height, so the active
                  // band reads as one continuous block and the per-lane
                  // playhead segments line up edge to edge.
                  alignItems: 'stretch',
                  width: 'max-content',
                  minWidth: '100%',
                }}
              >
                {lanes.map((lane) => {
                  const isHighlighted = lane.threadId === highlightedThreadId;
                  return (
                    <TimelineLaneRow
                      key={lane.threadId}
                      lane={lane}
                      isHighlighted={isHighlighted}
                      laneAxisWidth={laneAxisWidth}
                      messagePxByUuid={messagePxByUuid}
                      playheadX={playheadX}
                    />
                  );
                })}
              </ul>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
