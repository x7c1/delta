import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from 'react';
import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useThreadsMessagesQueries } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import {
  buildGlobalXMap,
  buildLaneRenderItems,
  buildLargeSortedMessages,
  buildSortedMessages,
  buildTimelineLanes,
  computeTimeRange,
  findNearestMessageIndex,
  type LaneCluster,
  type SortedMessage,
  type TimelineDot,
  type TimelineDotSize,
} from './timelineLanes';

/**
 * localStorage key for the timeline footer's expanded/collapsed state. Per
 * device, not per session — the user's preference travels across sessions.
 */
export const TIMELINE_EXPANDED_STORAGE_KEY = 'delta.thread-timeline-overlay.expanded';

/**
 * Rolling window (ms) over which wheel-event |delta| magnitudes accumulate
 * so a vigorous spin advances more steps than a leisurely turn. Each event's
 * normalized contribution sticks around for this duration; once nothing
 * fires for longer than the window the accumulator resets to 0 on the next
 * event, so an unrelated later flick always starts fresh at the slowest
 * step.
 *
 * Tuned to ~250 ms: short enough that two deliberate but slow turns stay
 * independent (each at the lowest step), long enough that a multi-notch
 * spin compounds into the higher staircase buckets while the user's fingers
 * are still in motion. Exported so a test can drive the window timing
 * without sleeping wall-clock time.
 */
export const WHEEL_VELOCITY_WINDOW_MS = 250;

/**
 * Upper bound (px) on a single wheel event's |delta| contribution to the
 * accumulator. Trackpads emit many small pixel-mode events per flick (often
 * 5–20 px each); without per-event clamping a single inertial burst would
 * pile up hundreds of px and explode straight into the top staircase
 * bucket. The clamp sits at one mouse-wheel notch (~100 px on Linux /
 * Chrome) so a single notch always contributes at most one notch's worth
 * of acceleration regardless of the source device.
 */
export const WHEEL_PER_EVENT_CLAMP_PX = 100;

/**
 * `WheelEvent.deltaMode` indicates the unit of `deltaY` / `deltaX`. Pixel
 * mode (0) is the trackpad / high-resolution-mouse default and needs no
 * conversion; line mode (1) and page mode (2) report small integer counts
 * that must be scaled to a pixel-equivalent magnitude before clamping so
 * cross-device behaviour stays consistent. The multipliers are deliberate
 * approximations — one line ≈ 40 px, one page ≈ 800 px — matching the
 * staircase's notch-sized thresholds.
 */
export const WHEEL_DELTA_LINE_PX = 40;
export const WHEEL_DELTA_PAGE_PX = 800;

/**
 * Velocity → step-count staircase, encoded as descending-threshold entries
 * (highest bucket first so a top-down walk picks the first match). Each
 * entry maps "cumulative |delta| at least this large within the rolling
 * window" → "number of large-message steps to take on this wheel event".
 *
 * The first acceleration bucket sits strictly above one notch's worth of
 * accumulated |delta| ({@link WHEEL_PER_EVENT_CLAMP_PX} = 100 px), so a
 * single leisurely notch ALWAYS lands in the slowest bucket (1 step) — the
 * user can land on the immediate prev/next message. Acceleration only kicks
 * in once a second notch arrives inside the rolling window (cum ≥ 200), at
 * which point the staircase compounds: 2 / 3 / 5 / 8 steps at the 200 / 400
 * / 700 / 1100 px thresholds. A sustained vigorous spin still traverses a
 * long session in a handful of turns, but the bug where the very first
 * notch already jumped two messages is gone.
 *
 * Exported so tests can assert the calculator's behaviour against the
 * same thresholds the live UI uses, without duplicating magic numbers.
 */
export const WHEEL_STEP_STAIRCASE: ReadonlyArray<{
  readonly minCumulativePx: number;
  readonly steps: number;
}> = [
  { minCumulativePx: 1100, steps: 8 },
  { minCumulativePx: 700, steps: 5 },
  { minCumulativePx: 400, steps: 3 },
  { minCumulativePx: 200, steps: 2 },
  { minCumulativePx: 0, steps: 1 },
];

/**
 * Convert a raw `WheelEvent.deltaY` magnitude in the event's native
 * `deltaMode` to a pixel-equivalent magnitude, clamped to
 * {@link WHEEL_PER_EVENT_CLAMP_PX}. The conversion lets line / page-mode
 * scrolls compete on the same staircase as pixel-mode events; the clamp
 * bounds a single trackpad event so an inertial burst cannot explode the
 * accumulator.
 *
 * Exported for unit testing — the wheel handler is the only runtime caller.
 */
export function normalizeWheelDeltaPx(
  deltaMagnitude: number,
  deltaMode: number,
): number {
  const abs = Math.abs(deltaMagnitude);
  let scaled: number;
  if (deltaMode === 1) {
    scaled = abs * WHEEL_DELTA_LINE_PX;
  } else if (deltaMode === 2) {
    scaled = abs * WHEEL_DELTA_PAGE_PX;
  } else {
    scaled = abs;
  }
  return Math.min(scaled, WHEEL_PER_EVENT_CLAMP_PX);
}

/**
 * Map a cumulative |delta| (px, within the rolling window) to a step count
 * by walking {@link WHEEL_STEP_STAIRCASE} from the top bucket down — the
 * first entry whose threshold the cumulative value meets wins. Always
 * returns at least 1: any nonzero wheel input deserves at least one step,
 * so the user can always land on the immediate prev/next message with a
 * single slow notch (with the staircase's first acceleration bucket sitting
 * strictly above one notch's clamped contribution, a leisurely turn never
 * trips a higher bucket). Exported for unit testing.
 */
export function stepsForCumulativePx(cumulativePx: number): number {
  for (const entry of WHEEL_STEP_STAIRCASE) {
    if (cumulativePx >= entry.minCumulativePx) {
      return Math.max(1, entry.steps);
    }
  }
  // Defensive: the last entry's threshold is 0 so the loop above always
  // returns; keep an explicit fallback so a future edit that drops the
  // 0-threshold entry still degrades gracefully to a single step.
  return 1;
}

/**
 * CSS transition duration (ms) for the playhead's `left` animation. Short
 * enough that the user always feels the playhead is "tracking" their input,
 * long enough that the discrete step between adjacent messages does not
 * teleport jarringly.
 */
const PLAYHEAD_TRANSITION_MS = 100;

/**
 * CSS class added to a transcript message article right after a wheel/click
 * jump scrolls it into view, then removed after the highlight fades so the
 * eye spots where the navigation landed. The class drives a one-shot
 * background-color fade on the inner message bubble — no overlay layer, the
 * highlight lands directly on the element MessageItem paints with the rest
 * color. Matches the rule under `.delta-timeline-jump-highlight` in
 * index.css.
 *
 * Exported so a test can assert the class is applied without depending on
 * the literal string in two places.
 */
export const TIMELINE_JUMP_HIGHLIGHT_CLASS = 'delta-timeline-jump-highlight';

/**
 * Duration (ms) the {@link TIMELINE_JUMP_HIGHLIGHT_CLASS} stays applied. The
 * CSS animation under that class fades the temporary amber background back
 * to fully transparent over this window, exposing the bubble's resting
 * color; once the class is removed the bubble is at its normal color and a
 * subsequent jump to the same message can re-apply the highlight cleanly.
 * Tuned slightly longer than the v6 flash (~1.5 s) so the fade reads as a
 * smooth transition, not a quick blink.
 */
export const TIMELINE_JUMP_HIGHLIGHT_MS = 1500;

/**
 * Debounce window (ms) for pane-scroll → playhead-follow updates. The
 * conversation pane's IntersectionObserver fires bursts of entries as the
 * user pans (every margin crossed); without debouncing the playhead would
 * thrash and consume CPU while scrolling a long thread. ~100 ms is short
 * enough that the playhead always feels "live" against the scroll and long
 * enough to collapse a burst of overlapping IO callbacks into one commit.
 * Exported so tests can drive the timing explicitly.
 */
export const PANE_SCROLL_DEBOUNCE_MS = 100;

/**
 * Cool-down window (ms) after a programmatic scroll (timeline → pane) during
 * which pane-scroll → playhead updates are suppressed. The browser keeps
 * firing IO entries while a `scrollIntoView` is animating into place, and
 * those entries would otherwise feed straight back into the playhead and
 * re-trigger a thread switch — the classic ping-pong. 200 ms is comfortably
 * longer than a typical jump animation plus a debounce burst, while still
 * short enough that a genuine user scroll moments after a jump is honoured.
 * Exported so tests can drive the timing explicitly.
 */
export const PANE_SCROLL_PROGRAMMATIC_GUARD_MS = 200;

/**
 * IntersectionObserver `threshold` for the pane-scroll observer. A single
 * 0-fraction entry is all we need: the moment any pixel of a message enters
 * or leaves the root viewport, the callback fires with the latest
 * `isIntersecting` state, which is enough to pick the topmost-visible
 * message. A multi-threshold list would generate redundant callbacks for
 * the same "the message is partially visible" state. Exported so tests can
 * assert the wiring without re-deriving the magic number.
 */
export const PANE_SCROLL_OBSERVER_THRESHOLD = 0;

/**
 * Read the persisted expanded preference; defaults to collapsed when no
 * preference has been saved yet or the storage layer is unavailable (SSR /
 * privacy-mode browsers).
 */
function readPersistedExpanded(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }
  try {
    return window.localStorage.getItem(TIMELINE_EXPANDED_STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

/**
 * Persist the expanded preference. Failures are swallowed so a quota error or
 * a disabled-storage browser never crashes the footer — the UI keeps working
 * in-memory for the session.
 */
function writePersistedExpanded(expanded: boolean): void {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    window.localStorage.setItem(
      TIMELINE_EXPANDED_STORAGE_KEY,
      expanded ? 'true' : 'false',
    );
  } catch {
    // Storage may be unavailable (quota, privacy mode); ignore.
  }
}

/**
 * Module-scoped store for the timeline expanded preference. Multiple
 * components read this same flag (the timeline itself, and the transcript
 * pane that switches its top-row layout so the Terminal button moves below
 * when the timeline is expanded). A click on the toggle must update every
 * subscriber on the same tick — per-component `useState` would only sync via
 * the `storage` event, which does not fire on same-document writes. A small
 * pub-sub keeps every subscriber in lockstep without pulling a full store in
 * for one boolean.
 */
let timelineExpandedCache: boolean | null = null;
const timelineExpandedListeners = new Set<(value: boolean) => void>();

function getTimelineExpanded(): boolean {
  if (timelineExpandedCache === null) {
    timelineExpandedCache = readPersistedExpanded();
  }
  return timelineExpandedCache;
}

function setTimelineExpanded(next: boolean): void {
  timelineExpandedCache = next;
  writePersistedExpanded(next);
  for (const listener of timelineExpandedListeners) {
    listener(next);
  }
}

/**
 * Drop the in-memory cache so a test that clears `localStorage` between cases
 * starts from a fresh read rather than the previous case's last write.
 * Production code does not need this — the cache lives for the page session.
 */
export function resetTimelineExpandedForTests(): void {
  timelineExpandedCache = null;
}

/**
 * Expanded/collapsed state for the timeline footer, persisted to localStorage
 * so the preference survives reloads. Initial state is collapsed when no
 * preference has been saved. All callers share one value (see the
 * module-scoped store above), so toggling in one place updates the others on
 * the same tick. Exported so tests and the transcript pane (which switches
 * its top-row layout on the same flag) can read and drive the toggle.
 */
export function useTimelineExpanded(): [boolean, () => void] {
  const [expanded, setExpanded] = useState<boolean>(() => getTimelineExpanded());
  useEffect(() => {
    const listener = (value: boolean) => setExpanded(value);
    timelineExpandedListeners.add(listener);
    // Sync to the current value in case it changed between render and
    // subscribe (e.g. another consumer toggled it in the same render pass).
    setExpanded(getTimelineExpanded());
    return () => {
      timelineExpandedListeners.delete(listener);
    };
  }, []);
  const toggle = useCallback(() => {
    setTimelineExpanded(!getTimelineExpanded());
  }, []);
  return [expanded, toggle];
}

/**
 * CSS selector matching a transcript message article by uuid. The selector
 * is anchored to the `<article>` tag so it never matches the timeline's own
 * dots or clusters, which stamp the SAME `data-message-uuid` value on a
 * `<span>` (see {@link TimelineDotMark} / {@link TimelineClusterMark}).
 *
 * Without the tag anchor a `[data-message-uuid="X"]` query rooted at the
 * conversation pane's scroll container hits the timeline span first
 * (DOM-pre-order — the top-region floating cards render before the
 * message list), so `scrollIntoView` lands on the already-visible dot
 * (no-op) and the pane-scroll IntersectionObserver observes the dot
 * instead of the article. Both regressions show up the moment the
 * timeline starts living inside the conversation pane's scroll
 * container — see TranscriptPane's `topRegion`.
 *
 * Exported so a regression test can pin the selector shape.
 */
export function articleMessageSelector(uuid: string): string {
  return `article[data-message-uuid="${CSS.escape(uuid)}"]`;
}

/**
 * CSS selector matching every transcript message article. The pane-scroll
 * IntersectionObserver iterates this set to track which article the user is
 * reading; the `<article>` tag anchor keeps the timeline's own dots — which
 * share the `data-message-uuid` attribute and (in the expanded state)
 * live in the same scroll container as the message articles — out of the
 * observation set.
 */
export const ALL_ARTICLES_SELECTOR = 'article[data-message-uuid]';

/**
 * Scroll the matching transcript message into view, aligned to the top of
 * the scrollable body. Scoped to the given container by uuid AND tag (see
 * {@link articleMessageSelector}), so neither a duplicate `data-message-uuid`
 * outside the transcript (e.g. in a portaled preview) nor the timeline's
 * own dots can misdirect the jump.
 *
 * Using `block: 'start'` rather than the v6 `block: 'center'` means the
 * destination message becomes the first line the eye reads on the next
 * paint — a centred message wastes half the viewport above the line the
 * user just asked to jump to. The transcript's top region overlay (the
 * collapsed-state breadcrumb and {Thread + Terminal} floating cards)
 * would otherwise hide the top of the article; the `scroll-margin-top`
 * rule on `article[data-message-uuid]` (driven by the live overlay
 * height via `--delta-top-region-reserve` — see index.css and
 * TranscriptPane) shifts the landing position down by that height so
 * the article lands just below the overlay row.
 *
 * The `scrollIntoView` call is guarded against environments where it is
 * unavailable (jsdom does not implement it on every element by default), so
 * an automatic jump driven by the playhead settle cannot crash unrelated
 * tests that render the overlay but never opted into a `scrollIntoView` stub.
 */
export function scrollMessageIntoView(
  container: HTMLElement | null,
  uuid: string,
): void {
  if (!container) {
    return;
  }
  const target = container.querySelector(articleMessageSelector(uuid));
  if (target && typeof target.scrollIntoView === 'function') {
    target.scrollIntoView({ block: 'start' });
  }
}

/**
 * Briefly mark the matching transcript message with the jump-highlight class
 * so the eye spots where the navigation landed. The class sets a temporary
 * background-color on the bubble and the CSS transition fades it back to the
 * resting color — no overlay layer, the highlight lands directly on the
 * message body. Scoped to the given container AND the `<article>` tag (see
 * {@link articleMessageSelector}) so neither a duplicate uuid in a portaled
 * preview nor the timeline's own dots steal the highlight (a dot
 * highlighting amber would be confusing and would mask the missing
 * article-level highlight).
 *
 * Removing the class after {@link TIMELINE_JUMP_HIGHLIGHT_MS} lets a
 * subsequent jump to the same message re-apply the highlight from rest.
 * The cleanup uses `window.setTimeout` so the call is no-op safe under SSR
 * or jsdom without native timers (a missing `setTimeout` would just skip
 * the highlight).
 *
 * Returns a cancel handle the caller can fire to clear the class early if
 * the component unmounts or a superseding jump arrives before the timer.
 */
export function highlightMessageJump(
  container: HTMLElement | null,
  uuid: string,
): () => void {
  if (!container) {
    return () => undefined;
  }
  const target = container.querySelector(articleMessageSelector(uuid));
  if (!target) {
    return () => undefined;
  }
  // Toggle the class off first so a repeat jump to the same message can
  // re-trigger the highlight cleanly (the CSS sets the temporary background
  // when the class is present; removing it first then adding it is what
  // restarts the visible transition).
  target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
  // Force a reflow so the remove + add cycle is two paint frames apart and
  // the transition restarts cleanly. Reading `offsetWidth` is the standard
  // trick; the assignment to a void variable keeps the side effect alive
  // under aggressive minifiers.
  void (target as HTMLElement).offsetWidth;
  target.classList.add(TIMELINE_JUMP_HIGHLIGHT_CLASS);
  if (typeof window === 'undefined' || typeof window.setTimeout !== 'function') {
    return () => target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
  }
  const handle = window.setTimeout(() => {
    target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
  }, TIMELINE_JUMP_HIGHLIGHT_MS);
  return () => {
    window.clearTimeout(handle);
    target.classList.remove(TIMELINE_JUMP_HIGHLIGHT_CLASS);
  };
}

/**
 * Maximum time (ms) {@link scheduleScrollAfterRender} polls for the target
 * message's element to appear in the DOM before giving up. The cross-lane
 * jump path switches the active thread first, then has to wait for the
 * conversation pane to re-render with the target thread's messages — which
 * can take several paint frames depending on the data layer (query refetch,
 * Suspense boundary, etc.). v10's single-rAF deferral was a no-op the
 * moment the re-render took more than one frame: `querySelector` returned
 * `null` and the scroll silently dropped. Polling across rAFs absorbs the
 * variable delay; the timeout caps the wait so a deleted message (or a
 * pane that genuinely never renders the uuid) cannot keep the loop running
 * forever.
 *
 * 1000 ms is roughly an order of magnitude above the worst observed
 * re-render delay in dogfooding — comfortable margin without feeling
 * stuck — and well below any "did the click do anything?" threshold a
 * human would notice. Exported so tests can assert the cap explicitly.
 */
export const SCROLL_DOM_READY_TIMEOUT_MS = 1000;

/**
 * Schedule {@link scrollMessageIntoView} to run as soon as the target uuid's
 * element appears in the DOM, so a preceding active-thread switch has time
 * to render the target thread's messages before the scroll fires. Polls
 * once per `requestAnimationFrame` until the element is present (or
 * {@link SCROLL_DOM_READY_TIMEOUT_MS} elapses), then scrolls and applies
 * the jump highlight in the same tick the element became visible.
 *
 * When the element never appears within the timeout the scroll is skipped
 * silently — the prior behaviour was a no-op `querySelector(null)` anyway,
 * so dropping the scroll on a missing target is not a behaviour change;
 * what we gain is the common case (re-render takes 2–N frames) actually
 * landing the scroll.
 *
 * Falls back to a zero-delay `setTimeout` when rAF is unavailable (older
 * test runners); in that fallback the wait is a single tick rather than
 * polled, matching the v10 deferral.
 *
 * The optional `onScroll` callback fires immediately before the
 * `scrollIntoView` call. Cross-lane callers use this to clear a guard flag
 * that suppresses pane → playhead sync during the DOM-ready wait; clearing
 * the flag right before the scroll lets the time-based programmatic-scroll
 * guard cover the remaining IO ripple window.
 *
 * The optional `onTimeout` callback fires when the polling gives up without
 * the element ever appearing. The cross-lane caller passes it to release
 * the in-flight counter — otherwise a missing article would leave
 * `crossLaneJumpInFlightCountRef` permanently armed, suppressing the
 * pane-scroll → playhead follower for the rest of the session. Symmetric
 * with `onScroll`, so the counter is balanced on EVERY outcome (success,
 * timeout, or cancel).
 *
 * Returns a cancel handle the caller can fire to abort the wait if the
 * component unmounts or another jump supersedes this one before the element
 * lands.
 */
export function scheduleScrollAfterRender(
  container: HTMLElement | null,
  uuid: string,
  onScroll?: () => void,
  onTimeout?: () => void,
): () => void {
  let highlightCancel: (() => void) | null = null;
  const run = () => {
    onScroll?.();
    scrollMessageIntoView(container, uuid);
    highlightCancel = highlightMessageJump(container, uuid);
  };
  if (
    typeof window !== 'undefined' &&
    typeof window.requestAnimationFrame === 'function' &&
    typeof window.performance !== 'undefined' &&
    typeof window.performance.now === 'function'
  ) {
    let cancelled = false;
    let rafHandle = 0;
    const start = window.performance.now();
    const tick = () => {
      if (cancelled) {
        return;
      }
      // Re-query each frame so a re-render that swapped the target node's
      // identity (or appended it for the first time) is picked up at the
      // earliest possible paint. The selector is article-anchored (see
      // {@link articleMessageSelector}) so the timeline's own dots — which
      // share the uuid attribute and may already be present in the same
      // container — never satisfy the wait early and cause a no-op scroll.
      const present =
        container !== null &&
        container.querySelector(articleMessageSelector(uuid)) !== null;
      if (present) {
        run();
        return;
      }
      if (window.performance.now() - start >= SCROLL_DOM_READY_TIMEOUT_MS) {
        // Polling timed out — invoke `onTimeout` so the cross-lane caller
        // can release the in-flight counter. Without this the counter
        // would stay > 0 forever and the IO follower would never sync the
        // playhead to a manual pane scroll again.
        onTimeout?.();
        return;
      }
      rafHandle = window.requestAnimationFrame(tick);
    };
    rafHandle = window.requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(rafHandle);
      highlightCancel?.();
    };
  }
  const handle = setTimeout(run, 0);
  return () => {
    clearTimeout(handle);
    highlightCancel?.();
  };
}

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

/** Lane row height in pixels. */
const LANE_HEIGHT_PX = 18;
/** Minimum width (in px) the lane axis reserves so a single-dot session is still scrubbable. */
const MIN_LANE_AXIS_PX = 240;

/**
 * Mark diameter in pixels, by size class.
 *
 * Round marks read more naturally as "speech turns" than rectangles, but a
 * single uniform size let dense lanes overlap into an illegible blur. Two
 * deliberate sizes solve both problems: the main-conversation turns (user +
 * assistant prose) are the larger circle, and the auxiliary marks (tool
 * calls, meta lines, question cards) are the smaller circle. The size delta
 * is small (just visible) so the lane still reads as one timeline rather than
 * two layers, while the smaller dot helps the eye filter out auxiliary marks
 * at a glance. Overlap is prevented at the layout level by the shared global
 * x map (see {@link buildGlobalXMap}), which pushes any neighbour that would
 * collide to the right by at least the sum of the two radii — so the marks
 * can stay solid-fill without any alpha or ring workaround.
 */
export const MARK_LARGE_PX = 6;
export const MARK_SMALL_PX = 4;
/**
 * Diameter (px) of a cluster mark — pinned to {@link MARK_SMALL_PX} so a
 * cluster is the same VISUAL size as a lone auxiliary dot, never the larger
 * headline-turn size. v10 nudged the inner fill a hair larger (5 px) to
 * make a cluster "stand out"; v11 reverted the fill to 4 px but layered a
 * 1 px outline outside the box for the same purpose. In dogfooding the
 * outline turned out to occupy the FULL 1 px on each side OUTSIDE the
 * disc (an `outline` is painted strictly outside the element's box, so the
 * 4 px disc became a 6 px outer footprint — identical in TOTAL width to a
 * 6 px main-role dot, and the user reported clusters as "large outlined
 * circles". The outline was therefore the same regression the size bump
 * had been, just dressed differently: it bumped the cluster's visible
 * footprint back into headline-turn territory.
 *
 * The cluster now renders as a plain small dot — same fill colour, same
 * outer extent — and "cluster-ness" is purely positional / interactive:
 * the dot still occupies its representative's x, the data attributes
 * still expose `data-cluster-member-count` for diagnostics, and a click
 * still snaps the playhead to the representative member. Losing the
 * visual distinction is a deliberate trade: a cluster that reads as a
 * normal small dot is honest about being one mark on the timeline; the
 * user cares about WHERE on the time axis it sits, not whether it
 * collapses 2 or 6 underlying messages.
 */
export const MARK_CLUSTER_PX = MARK_SMALL_PX;
/** Width reserved for the right-hand padding inside the lane area. */
const LANE_RIGHT_PAD_PX = 16;
/**
 * Tailwind class string for the collapsed-state toggle button. Mirrors the
 * Terminal button's shape exactly so the two buttons sit side-by-side in the
 * top region without one reading as a different control type. The Terminal
 * button lives in `WorkspaceScreen` and uses the same class chain — kept in
 * sync via `TERMINAL_TOGGLE_BUTTON_CLASS` over there.
 */
export const TIMELINE_TOGGLE_BUTTON_CLASS =
  'inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 shadow-md transition-colors hover:bg-slate-50';

/**
 * Glyph for the collapsed "Thread" toggle button: a stylised activity / signal
 * trace (a polyline of small peaks) so the button reads as a timeline at a
 * glance. Mirrors {@link TerminalIcon} (in `WorkspaceScreen`) in size and
 * stroke weight so the two buttons sit visually balanced in the same row.
 * Decorative — always `aria-hidden`, so the button's accessible name stays
 * its "Thread" label.
 */
function ThreadTimelineIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M3 12h3l3-7 4 14 3-7h5" />
    </svg>
  );
}

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

/**
 * Pixel diameter for a mark of the given size class. Fed to
 * {@link buildGlobalXMap} so the minimum spacing between two adjacent marks
 * is the average of their diameters — i.e. their summed radii — and the two
 * circles never paint into each other.
 */
function markDiameterPx(size: TimelineDotSize): number {
  return size === 'large' ? MARK_LARGE_PX : MARK_SMALL_PX;
}

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
 * stays under control. Clicking anywhere on the timeline jumps the active index
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
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const [expanded, toggle] = useTimelineExpanded();

  // N+1 is acceptable for MVP; the dedicated `all_threads=true` REST is
  // intentionally deferred. The query keys are shared with the focused
  // thread's `useThreadMessagesQuery`, so its messages are reused — no double
  // request.
  const threadIds = useMemo(() => threads.map((t) => t.id), [threads]);
  const messagesQueries = useThreadsMessagesQueries(client, threadIds);
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

  // The active message's index in `sortedMessages`. A fresh mount lands on
  // the latest message so a newly opened session highlights the most recent
  // utterance. `null` means there are no messages to land on yet.
  const [activeMessageIndex, setActiveMessageIndexState] = useState<number | null>(
    () => (sortedMessages.length === 0 ? null : sortedMessages.length - 1),
  );

  // A monotonically-increasing counter incremented on every user-driven
  // navigation that should trigger a JUMP (wheel scrub / axis click). The
  // thread-switch + scroll effect below uses it as a re-trigger so a
  // re-click at the playhead's current position (and thus the current
  // active index) still re-fires the jump — without it React would bail
  // out of the state set when the value is unchanged, swallowing the
  // user's intent. Bumped ONLY by the jump-driving setters; the pane →
  // playhead follower (Improvement 3) uses {@link userActedTick} instead
  // so it can pin the active index without re-firing a jump.
  const [scrubTick, setScrubTick] = useState(0);

  // A separate "the user has acted on the timeline at all" gate that
  // BOTH the jump path AND the pane-scroll follower bump. The auto-anchor
  // effect ("re-anchor to latest message") reads this instead of
  // {@link scrubTick} — without that change a pane scroll would commit a
  // new active index, then the next render's auto-anchor effect would
  // immediately reset it to the tail message, swallowing the follower's
  // update.
  const [userActedTick, setUserActedTick] = useState(0);
  const bumpUserActedTick = useCallback(() => {
    setUserActedTick((t) => t + 1);
  }, []);

  /**
   * Clamp an index into the valid range for the current sorted list and
   * commit it together with a tick bump that re-fires the navigation
   * effect. Centralising the clamp + tick here keeps the wheel and click
   * handlers from duplicating the same boilerplate.
   */
  const setActiveMessageIndex = useCallback(
    (next: number) => {
      if (sortedMessages.length === 0) {
        return;
      }
      const clamped = Math.max(0, Math.min(sortedMessages.length - 1, next));
      setScrubTick((tick) => tick + 1);
      bumpUserActedTick();
      setActiveMessageIndexState(clamped);
    },
    [sortedMessages.length, bumpUserActedTick],
  );

  /**
   * Commit a new active index that came from the pane-scroll → playhead
   * follower (Improvement 3), deliberately WITHOUT bumping {@link scrubTick}.
   * The tick is the re-trigger for the timeline → pane jump effect
   * (`scheduleScrollAfterRender` + `setActiveThread`); bumping it here would
   * close the ping-pong loop — the user's scroll would move the playhead,
   * which would scroll the pane, which would re-fire the observer, ad
   * infinitum.
   *
   * Skips when the index would not actually change (Object.is bail-out is
   * not enough — the IntersectionObserver fires duplicate "topmost is X"
   * entries while the user pans through X's reading band, and every commit
   * is one wasted render). The active thread is intentionally left alone
   * too: a pane scroll never switches lanes, because by definition the
   * pane is already inside the active subthread.
   */
  const setActiveMessageIndexFromPaneScroll = useCallback(
    (next: number) => {
      if (sortedMessages.length === 0) {
        return;
      }
      const clamped = Math.max(0, Math.min(sortedMessages.length - 1, next));
      let changed = false;
      setActiveMessageIndexState((prev) => {
        if (prev === clamped) {
          return prev;
        }
        changed = true;
        return clamped;
      });
      // Bump the "user has acted" gate only when we actually moved the
      // index — repeat IO entries for the same topmost message should not
      // keep flipping the auto-anchor gate on every burst.
      if (changed) {
        bumpUserActedTick();
      }
    },
    [sortedMessages.length, bumpUserActedTick],
  );

  // The active message itself, derived from the index — the single source of
  // truth for the playhead's x. There is no separately-stored fractional
  // position state: keeping the playhead a pure function of the active
  // message means continuous wheel inertia can never desync from a discrete
  // step.
  const activeMessage =
    activeMessageIndex !== null && activeMessageIndex < sortedMessages.length
      ? sortedMessages[activeMessageIndex]
      : null;
  // The playhead's x in pixels along the shared lane axis. Resolved through
  // the global map so the playhead and the mark under it always agree, even
  // when overlap mitigation pushed the mark off its ideal time-axis x.
  const playheadX =
    activeMessage === null ? 0 : messagePxByUuid.get(activeMessage.uuid) ?? 0;

  // Snapshot the active message into a ref so the navigation effect (which
  // depends on `scrubTick` alone) can read the latest pick without listing
  // `activeMessage` in its deps. Without this snapshot, a fresh
  // `sortedMessages` reference (e.g. from a background refetch) would swap
  // the `activeMessage` object identity and re-fire the auto-switch — which
  // is exactly the bug that overrode a user's Navigator click.
  const activeMessageRef = useRef(activeMessage);
  useEffect(() => {
    activeMessageRef.current = activeMessage;
  }, [activeMessage]);

  // Re-anchor to the latest message whenever a new one lands at the tail
  // while the user has not yet navigated. A navigation pins the active index
  // to whatever the user picked, and moving it on a fresh message would feel
  // like the timeline yanked away.
  //
  // Gated on {@link userActedTick} (not {@link scrubTick}) so a pane-scroll
  // follow update also counts as "user has navigated" — without that
  // distinction the follower would commit a new index and this effect
  // would yank it back to the tail on the very next render.
  useEffect(() => {
    if (userActedTick !== 0) {
      return;
    }
    setActiveMessageIndexState((prev) => {
      if (sortedMessages.length === 0) {
        return null;
      }
      const latest = sortedMessages.length - 1;
      return prev === latest ? prev : latest;
    });
  }, [sortedMessages, userActedTick]);

  // Keep the active index pointing at the SAME message across a
  // `sortedMessages` reference change (e.g. a background refetch landed a
  // new array with the same content, or a fresh message appended at the
  // tail). Without this the index would drift relative to the message the
  // user picked, and the wheel/click handlers would step from the wrong
  // anchor. A `null` index (no messages yet, or the picked message vanished)
  // falls back to the latest entry. Gated on {@link userActedTick} (same
  // reason as the auto-anchor effect above): any user action — jump OR
  // pane scroll — should preserve the picked message across refetches.
  useEffect(() => {
    if (userActedTick === 0) {
      return;
    }
    setActiveMessageIndexState((prev) => {
      if (sortedMessages.length === 0) {
        return null;
      }
      if (prev === null) {
        return sortedMessages.length - 1;
      }
      const prevUuid = activeMessageRef.current?.uuid;
      if (!prevUuid) {
        return prev;
      }
      const realigned = sortedMessages.findIndex((m) => m.uuid === prevUuid);
      if (realigned < 0) {
        // The picked message is no longer in the list (deleted, or the
        // session compacted). Clamp to the closest valid index.
        return Math.max(0, Math.min(sortedMessages.length - 1, prev));
      }
      return realigned;
    });
    // `activeMessageRef` is intentionally not in deps — it is a ref kept in
    // sync by another effect, and reading it here is just a cached lookup.
  }, [sortedMessages, userActedTick]);

  const activeThreadRef = useRef<ThreadId | null>(activeThreadId);
  useEffect(() => {
    activeThreadRef.current = activeThreadId;
  }, [activeThreadId]);

  // Hold a cancel handle for the pending render-frame scroll so a superseding
  // jump (or unmount) can suppress an in-flight scroll before it runs.
  const pendingScrollCancelRef = useRef<(() => void) | null>(null);
  useEffect(
    () => () => {
      pendingScrollCancelRef.current?.();
    },
    [],
  );

  // Timestamp (performance.now ms) marking the most recent timeline →
  // pane scroll we triggered programmatically. The pane-scroll observer
  // (Improvement 3) reads this and skips any update fired within
  // {@link PANE_SCROLL_PROGRAMMATIC_GUARD_MS} of it, so a same-lane jump's
  // own scrollIntoView cannot feed back into the playhead and re-trigger the
  // jump (the classic ping-pong). For cross-lane jumps this guard is not
  // sufficient — the scrollIntoView fires only after DOM-ready polling
  // completes, which can take longer than the guard window. Cross-lane jumps
  // use {@link crossLaneJumpInFlightRef} instead (see below).
  // `null` means "no recent programmatic scroll"; the observer treats
  // `null` as "free to update".
  const lastProgrammaticScrollAtRef = useRef<number | null>(null);
  const markProgrammaticScroll = useCallback(() => {
    if (
      typeof performance !== 'undefined' &&
      typeof performance.now === 'function'
    ) {
      lastProgrammaticScrollAtRef.current = performance.now();
    } else {
      lastProgrammaticScrollAtRef.current = Date.now();
    }
  }, []);

  // State-based guard for cross-lane jumps. A counter (not a boolean) so a
  // burst of wheel scrubs that stacks multiple cross-lane jumps in flight is
  // tracked independently — the guard only releases when EVERY in-flight
  // jump has settled. With a single boolean the first jump's onScroll would
  // clear the flag while later jumps were still polling, opening the exact
  // race window the guard exists to close.
  //
  // The time-based {@link lastProgrammaticScrollAtRef} guard is insufficient
  // for cross-lane jumps because `scheduleScrollAfterRender` polls across
  // rAFs waiting for the target element — the actual `scrollIntoView` fires
  // only after the new subthread's re-render completes, which can exceed
  // {@link PANE_SCROLL_PROGRAMMATIC_GUARD_MS}. During that window:
  //   1. The IO effect re-runs (activeThreadId changed) and observes the
  //      new thread's articles.
  //   2. The IO fires its first-observation batch — the tail message is
  //      typically visible in a freshly-rendered pane.
  //   3. The debounce fires, the time-based guard is already expired, and
  //      `flush` commits the tail index — snapping the playhead to the
  //      right edge.
  //
  // Keeping the IO fully suppressed until the scroll lands ensures the
  // first-observation batch of the new thread is always ignored. Once the
  // scroll fires (or times out / is cancelled), the counter is decremented
  // so the user's subsequent manual scroll resumes normal pane → timeline
  // sync as soon as the last jump settles. Decrements are clamped at zero
  // so a duplicate cancel (cancel handle invoked twice, or invoked after
  // onScroll already fired) cannot wrap into a negative count that would
  // leave the guard permanently armed.
  const crossLaneJumpInFlightCountRef = useRef(0);
  const decrementCrossLaneInFlight = useCallback(() => {
    if (crossLaneJumpInFlightCountRef.current > 0) {
      crossLaneJumpInFlightCountRef.current -= 1;
    }
  }, []);

  // Pending cross-lane scroll target: set when the navigation effect kicks
  // off a cross-lane jump, cleared the moment the rAF poll OR the
  // synchronous {@link useLayoutEffect} below scrolls the destination
  // article into view. The layout effect runs on EVERY render after
  // commit (no deps); when its check finds the article AND the time-based
  // guard is still in the window (the rAF poll has not already fired its
  // onScroll for the same target), it scrolls — beating the rAF poll on
  // the common path where the per-thread cache lands the new articles
  // synchronously on the very next re-render. The rAF poll remains the
  // fallback for the slow path.
  //
  // CRITICAL: the layout effect must NOT release the cross-lane in-flight
  // counter on its own — releasing it before the IO observer for the new
  // thread has had its first-fire batch consumed would unblock the IO
  // flush and snap the playhead to the new thread's tail. The counter is
  // released exclusively by the rAF poll's `onScroll` / `onTimeout`
  // callbacks (or by the cancel path), which run AFTER the IO has had a
  // chance to settle.
  const pendingCrossLaneTargetRef = useRef<{
    uuid: string;
    scrolled: boolean;
  } | null>(null);

  useEffect(() => {
    // Only react to navigation the user actually initiated: while
    // `scrubTick` sits at its initial 0 (a fresh mount with no wheel or
    // click yet), the automatic settle must not flip the active thread the
    // user chose elsewhere. After tick > 0 the effect still only fires when
    // the tick itself increments — the wheel/click handlers both bump it,
    // so a deliberate scrub always re-fires, but an incidental
    // `activeMessage` reference change (e.g. a new SortedMessage[] from a
    // background refetch) does not.
    if (scrubTick === 0) {
      return;
    }
    const current = activeMessageRef.current;
    if (current === null) {
      return;
    }
    pendingScrollCancelRef.current?.();
    pendingScrollCancelRef.current = null;
    const container = conversationBodyRef.current;
    if (current.threadId === activeThreadRef.current) {
      // Same lane: the target message is already in the DOM, scroll right
      // away. No frame deferral, no thread switch. The highlight fires
      // right after the scroll so the eye spots where the playhead landed.
      //
      // The time-based guard is stamped IMMEDIATELY before the scroll fires
      // (not at the top of the effect): the guard window must start ticking
      // from the moment the IO ripples actually begin, otherwise a slow
      // re-trigger could let the window expire before the scroll lands.
      // For a same-lane jump the two moments are the same tick — this is
      // straightforward — but keeping the stamp adjacent to the scroll keeps
      // the cross-lane path's analogous discipline (see below) easy to read.
      markProgrammaticScroll();
      scrollMessageIntoView(container, current.uuid);
      pendingScrollCancelRef.current = highlightMessageJump(
        container,
        current.uuid,
      );
      return;
    }
    // Cross-lane jump: raise the in-flight counter BEFORE switching the
    // active thread so the IO effect (which re-runs on activeThreadId
    // change) sees the guard already up when it first-fires its observation
    // batch on the new thread's articles. The counter is decremented via the
    // `onScroll` callback passed to scheduleScrollAfterRender — right before
    // scrollIntoView fires, at which point the time-based guard
    // (markProgrammaticScroll, stamped at the same moment) takes over
    // covering the remaining IO ripple window. If the jump is cancelled
    // before the element lands (superseding jump or unmount),
    // cancelWithCountClear decrements the counter immediately.
    //
    // CRITICAL: the time-based guard MUST be stamped here in the onScroll
    // callback — NOT at jump-trigger time — because scheduleScrollAfterRender
    // can poll for many frames waiting for the new thread's re-render. If we
    // stamped the guard at trigger time the window could expire before the
    // scroll lands, leaving the post-scroll IO ripples completely unguarded.
    // That was the residual tail-jump race that survived the v12 fix.
    crossLaneJumpInFlightCountRef.current += 1;
    // `released` guards against double-release: the three paths that may
    // settle a cross-lane jump's counter (rAF poll's onScroll, rAF poll's
    // onTimeout, the cancel handle firing on a superseding jump /
    // unmount) share this flag, so the counter is decremented at most
    // once per jump. The synchronous {@link useLayoutEffect} below does
    // NOT release the counter — it only performs the scroll and marks
    // the target as already-scrolled so the rAF poll's onScroll skips
    // the duplicate scroll. The counter release is therefore always
    // gated through the poll path or the cancel path, keeping the IO
    // observer's first-fire batch suppressed until the time-based guard
    // window naturally takes over (the poll's onScroll stamps it as it
    // releases).
    let released = false;
    const releaseOnce = () => {
      if (released) {
        return;
      }
      released = true;
      decrementCrossLaneInFlight();
    };
    // Stamp the pending target BEFORE setActiveThread so the synchronous
    // useLayoutEffect below — which fires on the very next render after
    // the active-thread switch propagates — can scroll the article into
    // view as soon as it appears in the DOM, without waiting for the
    // next paint frame's rAF poll. The poll remains in charge of
    // releasing the counter (and stamping the time-based guard) so the
    // IO guard chain remains intact.
    pendingCrossLaneTargetRef.current = {
      uuid: current.uuid,
      scrolled: false,
    };
    setActiveThread(current.threadId);
    const rawCancel = scheduleScrollAfterRender(
      container,
      current.uuid,
      () => {
        // onScroll: the rAF poll found the article. Stamp the time-based
        // guard now (so its 200ms window starts ticking from the moment
        // the IO ripples will arrive), release the state-based counter,
        // and clear the pending target so the useLayoutEffect does not
        // re-fire on subsequent renders.
        //
        // NOTE: the scroll itself is already done if the useLayoutEffect
        // beat the poll to the article. `scrollMessageIntoView` is
        // idempotent — calling it a second time on an already-aligned
        // element is a no-op scroll. So we let `scheduleScrollAfterRender`'s
        // own scroll fire either way; only the side effects (guard,
        // counter, highlight) are gated.
        pendingCrossLaneTargetRef.current = null;
        markProgrammaticScroll();
        releaseOnce();
      },
      () => {
        // onTimeout: polling gave up without the article appearing.
        // Release the counter so the IO follower is not permanently
        // suppressed — same {@link releaseOnce} guard so a subsequent
        // cancel cannot double-decrement. The pending target is also
        // cleared so the useLayoutEffect does not keep trying.
        pendingCrossLaneTargetRef.current = null;
        releaseOnce();
      },
    );
    // Wrap the cancel so the counter is also released if the scroll is
    // aborted (superseded by another jump or unmount) — otherwise a stacked
    // jump's counter would never decrement and the guard would stay armed
    // indefinitely.
    const cancelWithCountClear = () => {
      pendingCrossLaneTargetRef.current = null;
      releaseOnce();
      rawCancel();
    };
    pendingScrollCancelRef.current = cancelWithCountClear;
    // `scrubTick` is the re-trigger AND the gate: a fresh scrub bumps the
    // tick, re-fires this effect, and re-emits the (possibly identical)
    // navigation intent. A re-click at the same x bumps the tick even when
    // the active index does not move, so a stale scroll position is still
    // corrected — but a tick-less re-render never sneaks in an auto-switch
    // that the user did not ask for.
  }, [
    scrubTick,
    conversationBodyRef,
    setActiveThread,
    markProgrammaticScroll,
    decrementCrossLaneInFlight,
  ]);

  // Synchronous cross-lane scroll path: runs after every commit (the
  // navigation effect set {@link pendingCrossLaneTargetRef} and called
  // `setActiveThread`; the parent re-renders the conversation pane with
  // the new thread's messages; this layout effect fires before the next
  // paint and checks whether the target article has landed in the body).
  // If yes, scroll synchronously and MARK the pending target as scrolled
  // — beating the {@link scheduleScrollAfterRender} rAF poll to the
  // article on the common path (per-thread cache hit, so the new
  // articles render on the very next commit). The poll keeps polling
  // either way; once IT finds the article (next paint), its `run` will
  // re-scroll (no-op since we already scrolled here) AND fire the
  // counter release + highlight via the standard pipeline.
  //
  // CRITICAL: this layout effect MUST NOT touch
  // {@link pendingScrollCancelRef} (which is the rAF poll's cancel
  // handle, wrapped to release the in-flight counter) and MUST NOT
  // release the counter directly. Releasing the counter here would
  // unblock the IO follower BEFORE the new IO observer's first-fire
  // batch has been consumed — the flush would then commit the new
  // thread's tail message and snap the playhead to the wrong place.
  // The counter release stays exclusively with the rAF poll's
  // `onScroll` / `onTimeout` (or the cancel path), which fire on the
  // NEXT paint frame — by which time the IO observer's first-fire
  // batch has been collected and the time-based programmatic-scroll
  // guard the poll stamps covers the rest of the debounce window.
  useLayoutEffect(() => {
    const pending = pendingCrossLaneTargetRef.current;
    if (!pending || pending.scrolled) {
      return;
    }
    const container = conversationBodyRef.current;
    if (!container) {
      return;
    }
    const target = container.querySelector(articleMessageSelector(pending.uuid));
    if (!target) {
      // Article has not landed yet; the rAF poll keeps watching. No-op
      // this commit; the next render will try again.
      return;
    }
    pending.scrolled = true;
    if (typeof (target as HTMLElement).scrollIntoView === 'function') {
      (target as HTMLElement).scrollIntoView({ block: 'start' });
    }
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
  const activeMessageIndexRef = useRef(activeMessageIndex);
  useEffect(() => {
    activeMessageIndexRef.current = activeMessageIndex;
  }, [activeMessageIndex]);
  const sortedMessagesRef = useRef(sortedMessages);
  useEffect(() => {
    sortedMessagesRef.current = sortedMessages;
  }, [sortedMessages]);
  const largeSortedMessagesRef = useRef(largeSortedMessages);
  useEffect(() => {
    largeSortedMessagesRef.current = largeSortedMessages;
  }, [largeSortedMessages]);

  // Rolling-window accumulator for wheel-event |delta|. Each entry is a
  // single wheel event's normalized px contribution paired with the
  // timestamp it landed on; the wheel handler evicts entries older than
  // {@link WHEEL_VELOCITY_WINDOW_MS} before reading the sum, so a multi-
  // notch spin compounds while the user's fingers are still moving but an
  // unrelated later flick always starts fresh at the slowest staircase
  // bucket. The accumulator's role replaces v4's hard cooldown: a long
  // session traverses in a handful of vigorous turns instead of dozens.
  const wheelWindowRef = useRef<Array<{ atMs: number; deltaPx: number }>>([]);

  useEffect(() => {
    const el = axisScrollRef.current;
    if (!el) {
      return;
    }
    const onWheel = (event: WheelEvent) => {
      // A wheel over a label cell must NOT scrub the timeline — labels
      // behave like normal page content. With the grid layout the labels
      // sit inside the same scroll container as the axis (so the sticky
      // label cell can pin to the left edge during horizontal pan), so
      // scope discrimination happens here on the event target rather than
      // by attaching to a smaller element. A wheel whose target is inside
      // a label cell falls through to the normal page scroll; everything
      // else in the scroll container (axis cells, the dots inside them,
      // the wrapper itself for dispatched test events) scrubs.
      const target = event.target as Element | null;
      if (target && target.closest('[data-testid="thread-timeline-lane-label"]')) {
        return;
      }
      // Suppress the page's vertical scroll while the wheel is over the
      // axis: the wheel belongs to the active-index step while it sits
      // here.
      event.preventDefault();
      // `deltaX` from horizontal trackpad scrolls is honoured too — the
      // user gets to pick whichever axis their device emits. Sum so a
      // diagonal gesture (rare but possible) reads as the combined intent.
      const rawDelta = event.deltaY + event.deltaX;
      if (rawDelta === 0) {
        return;
      }
      const total = sortedMessagesRef.current.length;
      const large = largeSortedMessagesRef.current;
      if (total === 0 || large.length === 0) {
        return;
      }
      const now =
        typeof performance !== 'undefined' &&
        typeof performance.now === 'function'
          ? performance.now()
          : Date.now();
      // Evict events older than the rolling window before accumulating so a
      // pause longer than the window resets the staircase — the next wheel
      // event starts the user at the slowest single-step bucket again.
      const cutoff = now - WHEEL_VELOCITY_WINDOW_MS;
      const window = wheelWindowRef.current;
      while (window.length > 0 && window[0].atMs <= cutoff) {
        window.shift();
      }
      // Normalize the per-event |delta| (deltaMode → px, clamped to one
      // notch) so a trackpad's inertial fan-out cannot explode the
      // accumulator while a deliberate mouse-wheel notch still registers
      // as one full notch's worth of acceleration.
      const contribPx = normalizeWheelDeltaPx(rawDelta, event.deltaMode);
      window.push({ atMs: now, deltaPx: contribPx });
      let cumulativePx = 0;
      for (const entry of window) {
        cumulativePx += entry.deltaPx;
      }
      const requestedSteps = stepsForCumulativePx(cumulativePx);
      // Wheel down (positive delta) → next message (newer); wheel up →
      // previous (older). Clamped to the ends — no wrap.
      const direction: 1 | -1 = rawDelta > 0 ? 1 : -1;
      const currentIndex = activeMessageIndexRef.current ?? total - 1;
      const currentMessage = sortedMessagesRef.current[currentIndex];
      // Walk the large-message subset `requestedSteps` times in the
      // requested direction, clamping at the ends. Walking the predicate
      // explicitly (instead of multiplying the cursor's `(timeMs, seq)` by
      // an estimated step size) keeps the staircase honest even when the
      // playhead currently sits on a small mark — the very first step
      // still snaps to the adjacent large neighbour.
      let cursor: SortedMessage | undefined = currentMessage;
      let landed: SortedMessage | null = null;
      for (let i = 0; i < requestedSteps; i += 1) {
        const next = pickNeighbourLargeMessage(large, cursor, direction);
        if (next === null) {
          break;
        }
        landed = next;
        cursor = next;
      }
      if (landed === null) {
        return;
      }
      const nextGlobalIndex = sortedMessagesRef.current.findIndex(
        (m) => m.uuid === landed.uuid,
      );
      if (nextGlobalIndex < 0 || nextGlobalIndex === currentIndex) {
        return;
      }
      setActiveMessageIndex(nextGlobalIndex);
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [expanded, setActiveMessageIndex]);

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

  // After a user-driven navigation, ensure the playhead is visible inside
  // the axis column's horizontal viewport. The lane axis is fixed-width so
  // this only matters when the viewport is narrower than the axis (e.g. a
  // narrow side panel); when the axis fits, no scroll is needed.
  useEffect(() => {
    if (scrubTick === 0) {
      return;
    }
    const scrollEl = axisScrollRef.current;
    if (!scrollEl || activeMessage === null) {
      return;
    }
    // The playhead's x is its position inside the axis (from the global x
    // map) plus the axis's left pad (so the leftmost large dot is not
    // clipped). The axis row starts at x=0 inside the axis scroll
    // container, so the left pad is the only adjustment needed (unlike
    // the v9 layout where the sticky label sat in the same scroll
    // container).
    const playheadInAxis =
      (messagePxByUuid.get(activeMessage.uuid) ?? 0) + LANE_LEFT_PAD_PX;
    const viewLeft = scrollEl.scrollLeft;
    const viewRight = viewLeft + scrollEl.clientWidth;
    if (playheadInAxis < viewLeft || playheadInAxis > viewRight) {
      scrollEl.scrollLeft = Math.max(0, playheadInAxis - scrollEl.clientWidth / 2);
    }
  }, [scrubTick, activeMessage, laneAxisWidth, messagePxByUuid]);

  // Pane scroll → playhead follow (Improvement 3). Observe each rendered
  // message article in the conversation pane; whichever one sits closest to
  // the viewport TOP is the "current" message and drives the playhead.
  // This is the bidirectional half of the sync: timeline → pane is already
  // wired by the navigation effect above; this is pane → timeline.
  //
  // Design notes:
  //  - We observe `IntersectionObserver` entries rather than wiring a
  //    raw `scroll` listener + `elementFromPoint`: IO is debounced by
  //    the browser, scopes naturally to "is this article on screen",
  //    and avoids the layout reads `elementFromPoint` forces.
  //  - The follower commits via {@link setActiveMessageIndexFromPaneScroll},
  //    which does NOT bump {@link scrubTick} — so this update never
  //    re-triggers the timeline → pane jump effect, breaking the
  //    ping-pong before it starts.
  //  - A programmatic-scroll guard (see {@link markProgrammaticScroll})
  //    further blocks the IO callbacks that fire during the timeline's
  //    own scrollIntoView. Without it, the jump's own scroll would still
  //    nudge `activeMessageIndex` between the jump's target and the
  //    message the scroll passes over en route.
  //  - Debounced commit collapses a burst of "topmost is X / topmost is
  //    Y" entries into one render while the user pans through.
  //
  // Observer is re-bound when the active thread changes (the pane swaps
  // its DOM) or when the sorted-messages list changes (a fresh message
  // landed and needs an `observe` call); guarded by `expanded` so a
  // collapsed timeline has no follower running.
  useEffect(() => {
    if (!expanded) {
      return;
    }
    if (typeof IntersectionObserver === 'undefined') {
      // jsdom in older test runners may lack IO; the follower simply
      // does not run there. Pane → timeline is purely additive UX, so
      // skipping it is harmless.
      return;
    }
    const container = conversationBodyRef.current;
    if (!container) {
      return;
    }
    // Build a uuid → global-index lookup once per re-bind, so the per-
    // entry callback work stays O(1).
    const indexByUuid = new Map<string, number>();
    sortedMessages.forEach((m, i) => indexByUuid.set(m.uuid, i));
    // The set of articles currently intersecting the viewport. We commit
    // the topmost-visible by smallest `boundingClientRect.top` per
    // debounce tick — that is the message the user is most likely reading.
    const intersecting = new Map<string, number>();
    let debounceHandle: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      debounceHandle = null;
      // State-based guard for cross-lane jumps: at least one thread switch
      // has fired but its DOM-ready scroll has not yet — the IO's first-
      // observation batch on the new thread's articles must not commit the
      // tail as the active index. A counter (not a boolean) so a stacked
      // burst of cross-lane jumps suppresses the IO until EVERY in-flight
      // jump has settled; the counter is decremented by
      // scheduleScrollAfterRender's onScroll callback (right before
      // scrollIntoView fires) or by the cancel handle (superseded jump /
      // unmount), so it can never permanently block pane → timeline sync.
      if (crossLaneJumpInFlightCountRef.current > 0) {
        return;
      }
      // Time-based guard: a same-lane programmatic scroll (scrollIntoView)
      // is still settling. Honouring the IO entries here would feed the
      // jump's own scroll ripple back into the playhead.
      const guardedAt = lastProgrammaticScrollAtRef.current;
      if (guardedAt !== null) {
        const now =
          typeof performance !== 'undefined' &&
          typeof performance.now === 'function'
            ? performance.now()
            : Date.now();
        if (now - guardedAt < PANE_SCROLL_PROGRAMMATIC_GUARD_MS) {
          return;
        }
        // Outside the window — clear so a true freshly-arriving scroll
        // (no jump in between) gets honoured immediately on its first
        // entry rather than queueing a redundant null check.
        lastProgrammaticScrollAtRef.current = null;
      }
      if (intersecting.size === 0) {
        return;
      }
      // Pick the topmost visible: smallest viewport-top wins. Ties (rare,
      // and only if two articles share an exact y) fall back to smallest
      // global index so the choice is deterministic.
      let bestUuid: string | null = null;
      let bestTop = Number.POSITIVE_INFINITY;
      let bestIndex = Number.POSITIVE_INFINITY;
      for (const [uuid, top] of intersecting) {
        const idx = indexByUuid.get(uuid);
        if (idx === undefined) {
          continue;
        }
        if (top < bestTop || (top === bestTop && idx < bestIndex)) {
          bestTop = top;
          bestIndex = idx;
          bestUuid = uuid;
        }
      }
      if (bestUuid === null) {
        return;
      }
      const targetIndex = indexByUuid.get(bestUuid);
      if (targetIndex === undefined) {
        return;
      }
      setActiveMessageIndexFromPaneScroll(targetIndex);
    };
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const el = entry.target as HTMLElement;
          const uuid = el.getAttribute('data-message-uuid');
          if (!uuid) {
            continue;
          }
          if (entry.isIntersecting) {
            intersecting.set(uuid, entry.boundingClientRect.top);
          } else {
            intersecting.delete(uuid);
          }
        }
        if (debounceHandle !== null) {
          clearTimeout(debounceHandle);
        }
        debounceHandle = setTimeout(flush, PANE_SCROLL_DEBOUNCE_MS);
      },
      {
        root: container,
        threshold: PANE_SCROLL_OBSERVER_THRESHOLD,
      },
    );
    // Observe every article that carries a `data-message-uuid` inside the
    // pane. The transcript's MessageItem stamps that attribute on every
    // rendered turn, so a single querySelectorAll covers all message
    // bodies. The selector is article-anchored ({@link ALL_ARTICLES_SELECTOR})
    // so the timeline's own dots and clusters — which share the
    // `data-message-uuid` attribute and (in the expanded state) live in
    // the same scroll container as the message articles — are NOT
    // observed; if they were, the dots would always win the
    // topmost-visible race (they sit at the top of the viewport above
    // the conversation) and the playhead would never follow the user's
    // conversation-pane scroll. The query is repeated below via a
    // `MutationObserver` so articles that appear after this initial pass
    // (streaming arrival, a background refetch) are picked up without
    // remounting the IO.
    const observed = new Set<Element>();
    const observeMatching = () => {
      const targets = container.querySelectorAll(ALL_ARTICLES_SELECTOR);
      for (const target of targets) {
        if (observed.has(target)) {
          continue;
        }
        observed.add(target);
        observer.observe(target);
      }
    };
    observeMatching();
    // Track newly-added articles too: when a fresh message lands or the
    // pane re-renders the active thread's content, the new article needs
    // to be observed for the follower to track scroll past it.
    let mutationObserver: MutationObserver | null = null;
    if (typeof MutationObserver !== 'undefined') {
      mutationObserver = new MutationObserver(() => observeMatching());
      mutationObserver.observe(container, {
        childList: true,
        subtree: true,
      });
    }
    return () => {
      observer.disconnect();
      mutationObserver?.disconnect();
      if (debounceHandle !== null) {
        clearTimeout(debounceHandle);
      }
    };
  }, [
    expanded,
    conversationBodyRef,
    sortedMessages,
    setActiveMessageIndexFromPaneScroll,
    activeThreadId,
  ]);

  // The collapsed state is just a single button styled to match the
  // Terminal toggle that sits alongside it in the top region (see
  // {@link TIMELINE_TOGGLE_BUTTON_CLASS}). The expanded state grows
  // downward into a card whose chrome (border / shadow / background)
  // matches the breadcrumb and composer cards (the shared
  // {@link FLOATING_CARD_CLASS} family in `TranscriptPane`). Keeping the
  // collapsed toggle visually identical to Terminal — and the expanded
  // card visually identical to the other delta UI cards — is what makes
  // the timeline land in the top region without reading as a third style
  // of chrome.
  if (!expanded) {
    return (
      <button
        type="button"
        onClick={toggle}
        data-testid="thread-timeline-toggle"
        data-expanded="false"
        aria-expanded={false}
        aria-label="Thread"
        className={TIMELINE_TOGGLE_BUTTON_CLASS}
      >
        <ThreadTimelineIcon className="h-3.5 w-3.5" />
        Thread
      </button>
    );
  }
  return (
    <section
      data-testid="thread-timeline-overlay"
      data-expanded="true"
      className="select-none rounded-md border border-slate-300 bg-white text-xs text-slate-600 shadow-md"
      aria-label="Subthread timeline"
    >
      <button
        type="button"
        onClick={toggle}
        data-testid="thread-timeline-toggle"
        aria-expanded={expanded}
        className="flex w-full items-center justify-between gap-2 rounded-md px-3 py-1.5 text-left text-xs font-medium text-slate-700 transition-colors hover:bg-slate-50"
      >
        <span className="flex items-center gap-1.5">
          <ThreadTimelineIcon className="h-3.5 w-3.5" />
          Thread
        </span>
        <span aria-hidden="true" className="text-slate-400">
          ▾
        </span>
      </button>
      {expanded && (
        <div
          ref={bodyRef}
          data-testid="thread-timeline-body"
          // Outer wrapper: vertical scroll only. Horizontal scroll lives on
          // the axis-column wrapper below so the sticky label cells can pin
          // to the left edge as the user pans a wide axis.
          className="max-h-40 overflow-y-auto px-2 pb-1"
        >
          {lanes.length === 0 ? (
            <p className="px-1 py-1 text-[0.7rem] text-slate-400">
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
                  axis cell's fixed pixel height. Row gap mirrors the prior
                  `gap-0.5` between lane rows. */}
              <ul
                data-testid="thread-timeline-lane-grid"
                role="list"
                className="gap-y-0.5"
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
                //
                // `align-items: stretch` lets every grid item fill the
                // full row height instead of collapsing to its own
                // content height. Paired with `h-full` on the label
                // `<span>` and axis `<div>` cells, both halves of a lane
                // row paint the active-lane background across exactly
                // the same vertical extent — no pixel-level mismatch
                // from line-height / padding differences (v23
                // Improvement 2). The {@link LANE_HEIGHT_PX} `min-height`
                // on each row still floors the row height so a row with
                // no dots is not visually collapsed.
                //
                // `position: relative` makes the `<ul>` the containing
                // block for the unified playhead `<span>` below, which
                // is absolutely positioned to span every lane row as
                // one continuous vertical line (v23 Improvement 3).
                // The playhead is rendered ONCE here as a child of the
                // `<ul>` instead of once per lane, so there are no
                // visual gaps between row backgrounds where the
                // per-lane playhead used to break apart.
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'max-content 1fr',
                  gridAutoRows: `minmax(${LANE_HEIGHT_PX}px, auto)`,
                  alignItems: 'stretch',
                  position: 'relative',
                  width: 'max-content',
                  minWidth: '100%',
                }}
              >
                {lanes.map((lane) => {
                  const isHighlighted = lane.threadId === highlightedThreadId;
                  // The active-lane highlight (border-y + bg-slate-50) is
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
                  // class set. Doing it via className — `bg-white` resting,
                  // `bg-slate-50` active — keeps className as the single
                  // source of truth for the cell's visual state: an active
                  // sticky label paints `bg-slate-50` (matching the active
                  // axis cell so the band reads as one row), and an
                  // inactive sticky label paints `bg-white` (matching the
                  // body so axis dots cannot peek through).
                  //
                  // Both cells carry `h-full` (height: 100%) so they fill
                  // the full row height the grid's `align-items: stretch`
                  // hands them — the active-lane background paints as a
                  // single uniform band without the v23-era line-height
                  // / padding mismatch.
                  const highlightClasses = isHighlighted
                    ? 'border-y border-slate-200 bg-slate-50'
                    : 'border-y border-transparent';
                  const labelHighlightClasses = isHighlighted
                    ? 'border-y border-slate-200 bg-slate-50'
                    : 'border-y border-transparent bg-white';
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
                      key={lane.threadId}
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
                        // — `bg-white` resting, `bg-slate-50` active)
                        // prevents axis dots from peeking through during
                        // the pan; the z-index keeps the label above the
                        // axis line and dots. The background lives on the
                        // className (not inline) so the active highlight's
                        // `bg-slate-50` is the one that paints — an inline
                        // background would win over the class and would
                        // leave the sticky label white in the active state,
                        // breaking the visual continuity with the axis
                        // cell's highlight.
                        //
                        // `h-full` (height: 100%) plus the grid's
                        // `align-items: stretch` and per-row
                        // `minmax(LANE_HEIGHT_PX, auto)` makes the label
                        // paint across the exact same vertical extent
                        // as the axis cell next to it — the active-lane
                        // background bands line up without any pixel
                        // mismatch from line-height / padding differences.
                        // `flex items-center` is what now centres the
                        // text vertically inside the (taller) cell
                        // instead of the prior `line-height: LANE_HEIGHT_PX`
                        // trick, which presumed the cell's exact pixel
                        // height.
                        className={`flex h-full items-center truncate whitespace-nowrap rounded-sm py-0.5 pl-1 pr-2 font-mono text-[0.65rem] ${
                          lane.isMain ? 'text-slate-700' : 'text-slate-500'
                        } ${labelHighlightClasses}`}
                        style={{
                          position: 'sticky',
                          left: 0,
                          zIndex: 1,
                          minHeight: LANE_HEIGHT_PX,
                        }}
                      >
                        {lane.label}
                      </span>
                      <div
                        data-timeline-axis=""
                        data-thread-id={lane.threadId}
                        data-active={isHighlighted ? 'true' : 'false'}
                        // `h-full` (height: 100%) so the axis cell fills
                        // the row the grid's `align-items: stretch` gave
                        // it — matching the label cell next to it pixel-
                        // for-pixel so the active-lane background bands
                        // read as a single seamless row.
                        className={`relative h-full rounded-sm ${highlightClasses}`}
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
                          className="absolute top-1/2 h-px -translate-y-1/2 bg-slate-200"
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
                      </div>
                    </li>
                  );
                })}
                {/* Unified playhead: a single vertical line spanning the
                    full height of the lane grid, instead of one short
                    segment per lane (the v23 Improvement 3 fix for the
                    visual gaps the per-lane line left between
                    `gap-y-0.5`-separated rows).

                    Placement strategy: grid-place the playhead's
                    POSITIONING CONTEXT inside column 2 (the axis
                    column) spanning every row, so the playhead's
                    `left: playheadX + LANE_LEFT_PAD_PX` is relative
                    to the axis column's left edge — no need to
                    measure the (dynamically-sized) label column width
                    at runtime. The wrapper is `position: relative`
                    and `pointer-events-none` so it never intercepts
                    clicks meant for the axis cells underneath. The
                    inner `<span>` is `position: absolute` with
                    `height: 100%` so it stretches across every grid
                    row including the row gaps (the row gaps are
                    inside the wrapper, so 100% height covers them
                    too).

                    `z-index: 2` keeps the playhead above the axis
                    dots (which sit at the axis cells' default stack)
                    but below the sticky label cell (z-index 1 plus
                    its own opaque background) for the brief moment
                    of the leftmost pan, where the label slides over
                    the playhead's `left`. A short CSS transition on
                    `left` smooths the discrete step between adjacent
                    messages so the eye can follow the move. */}
                <div
                  data-testid="thread-timeline-playhead-track"
                  aria-hidden="true"
                  className="pointer-events-none relative"
                  style={{
                    gridColumn: '2',
                    gridRow: `1 / -1`,
                  }}
                >
                  <span
                    aria-hidden="true"
                    data-testid="thread-timeline-playhead"
                    className="pointer-events-none absolute top-0 w-px bg-indigo-500"
                    style={{
                      left: playheadX + LANE_LEFT_PAD_PX,
                      height: '100%',
                      zIndex: 2,
                      transition: `left ${PLAYHEAD_TRANSITION_MS}ms ease-out`,
                    }}
                  />
                </div>
              </ul>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

interface TimelineDotMarkProps {
  dot: TimelineDot;
  /**
   * Absolute x in pixels for this mark, resolved through the global x map
   * shared across every lane. The mark renders centred on this x so a
   * cross-lane playhead lands on the same column as the mark.
   */
  xPx: number;
}

/**
 * One mark within a lane. Rendered as a round speech-turn marker, colored by
 * author kind — user turns in blue, everything else in slate — and sized by
 * its role in the conversation: the main-conversation turns (user + Claude
 * prose) are the larger circle, auxiliary turns (tool calls, meta lines,
 * question cards) are the smaller circle. The tokens mirror `MessageItem`'s
 * bubble palette family so the timeline reads as the same conversation, just
 * compressed.
 *
 * Overlap is prevented at the layout level: the shared global x map (see
 * {@link buildGlobalXMap}) pushes any neighbour whose ideal time-axis x
 * would collide with the previous mark, so adjacent circles always clear
 * each other by at least the sum of their radii. The fill can therefore stay
 * solid — no alpha, no ring — and each mark reads as one disc.
 *
 * The mark is non-interactive: hover and click navigation flow through the
 * playhead alone, so a mark is purely a visual anchor.
 */
function TimelineDotMark({ dot, xPx }: TimelineDotMarkProps) {
  // Two-color scheme: user vs everything else. Mirrors `MessageItem`'s
  // bubble palette family — blue for user, slate for the assistant side.
  const colorClasses =
    dot.kind === 'user' ? 'bg-blue-500' : 'bg-slate-400';
  const diameter = dot.size === 'large' ? MARK_LARGE_PX : MARK_SMALL_PX;
  return (
    <span
      data-testid="thread-timeline-dot"
      data-message-uuid={dot.uuid}
      data-thread-id={dot.threadId}
      data-message-kind={dot.kind}
      data-message-size={dot.size}
      aria-hidden="true"
      className={`pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full ${colorClasses}`}
      style={{
        left: xPx,
        width: diameter,
        height: diameter,
      }}
    />
  );
}

interface TimelineClusterMarkProps {
  cluster: LaneCluster;
  /**
   * Absolute x in pixels for the cluster's representative (first member),
   * resolved through the global x map shared across every lane. The cluster
   * renders centred on this x; clicking near it snaps the playhead to the
   * representative message via the global nearest-message lookup.
   */
  xPx: number;
}

/**
 * A run of 2+ consecutive auxiliary marks (tool calls, meta lines, question
 * cards) collapsed into one visible disc. The cluster sits at the leftmost
 * member's x (its representative), so a left-to-right read of the lane stays
 * chronological, and a click that lands closest to it snaps the playhead to
 * the representative message via the global nearest-message lookup.
 *
 * The cluster renders at exactly the same diameter AND with no extra outline
 * vs. a lone small dot, so its total visual footprint equals
 * {@link MARK_CLUSTER_PX} px end-to-end — never the larger main-role
 * footprint. Earlier revisions tried a 5 px fill (v10) and then a 4 px fill
 * with a 1 px outline (v11) to make a cluster "stand out"; both produced a
 * 6 px outer disc indistinguishable from a 6 px main-role dot when the user
 * eyeballed the lane. The visual distinction is dropped on purpose: a
 * cluster behaves like a normal small dot to the eye, and the cluster
 * concept stays meaningful through (a) the representative x and (b) the
 * `data-cluster-member-count` attribute for downstream diagnostics and
 * tests. Click navigation keeps snapping to the representative member via
 * the global nearest-message lookup, with or without a visual cue.
 */
function TimelineClusterMark({ cluster, xPx }: TimelineClusterMarkProps) {
  return (
    <span
      data-testid="thread-timeline-cluster"
      data-message-uuid={cluster.representativeUuid}
      data-thread-id={cluster.threadId}
      data-cluster-member-count={cluster.memberCount}
      aria-hidden="true"
      // No outline, no ring, no border — just the slate-400 fill a lone
      // small assistant dot uses. The cluster's footprint equals a small
      // dot's exactly, never larger.
      className="pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-slate-400"
      style={{
        left: xPx,
        width: MARK_CLUSTER_PX,
        height: MARK_CLUSTER_PX,
      }}
    />
  );
}

/**
 * Find the next or previous `large` (main-conversation) message relative to
 * the playhead's current position in the timeline-sorted list of every
 * message. The playhead may currently sit on either a large mark (a click on
 * a user/Claude turn, or a prior wheel step) or a small one (a click on a
 * tool call). In either case we walk the large list, treating the playhead's
 * `(timeMs, seq)` as the cursor, so a wheel step always lands on the
 * adjacent main-conversation turn rather than the nearest tool call.
 *
 * Returns `null` when the requested neighbour is past the end of the list
 * (the wheel handler treats that as a clamp; the playhead does not wrap).
 */
function pickNeighbourLargeMessage(
  large: SortedMessage[],
  cursor: SortedMessage | undefined,
  step: 1 | -1,
): SortedMessage | null {
  if (large.length === 0) {
    return null;
  }
  if (!cursor) {
    // No current cursor — the wheel acts as "land on the latest large turn"
    // for a forward step and "land on the first" for a backward step.
    return step > 0 ? large[large.length - 1] : large[0];
  }
  // Locate the cursor's position in the large list. When the cursor IS a
  // large message, that position is the cursor itself; when it is small, the
  // position is between two large messages and we want the inserted index so
  // a forward step jumps to the next large, a backward step to the previous.
  const insertIndex = lowerBoundLargeIndex(large, cursor);
  const onLarge =
    insertIndex < large.length &&
    large[insertIndex].uuid === cursor.uuid;
  if (step > 0) {
    const nextIndex = onLarge ? insertIndex + 1 : insertIndex;
    return nextIndex < large.length ? large[nextIndex] : null;
  }
  // Backward: when standing on a large mark, the previous is `index - 1`;
  // when between, the "previous" large is `insertIndex - 1` (the one just
  // before the insertion point).
  const prevIndex = insertIndex - 1;
  return prevIndex >= 0 ? large[prevIndex] : null;
}

/**
 * Binary-search the insertion index for `cursor` in `large` under the same
 * `(timeMs, seq)` ascending order the sorted list is built with. When the
 * cursor IS a large message in the list, the returned index points at it (so
 * the caller can detect "we're standing on a large mark" by uuid equality);
 * otherwise it is the position where the cursor would be inserted to keep
 * the order — i.e. the index of the first large message strictly later than
 * the cursor.
 */
function lowerBoundLargeIndex(
  large: SortedMessage[],
  cursor: SortedMessage,
): number {
  let lo = 0;
  let hi = large.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const m = large[mid];
    const isBefore =
      m.timeMs < cursor.timeMs ||
      (m.timeMs === cursor.timeMs && m.seq < cursor.seq);
    if (isBefore) {
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return lo;
}
