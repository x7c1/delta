import {
  useCallback,
  useEffect,
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
  buildTimelineLanes,
  findActiveMessage,
  type TimelineDot,
} from './timelineLanes';

/**
 * localStorage key for the timeline footer's expanded/collapsed state. Per
 * device, not per session — the user's preference travels across sessions.
 */
export const TIMELINE_EXPANDED_STORAGE_KEY = 'delta.thread-timeline-overlay.expanded';

/**
 * Pixels of playhead movement per unit of wheel delta. One standard wheel
 * notch is `deltaY = 100` on most browsers, so this constant times 100 is the
 * pixel travel per notch.
 *
 * Tuned low so a single notch lands ~1–2 % of the axis width per notch
 * (≈3 px on a 240 px axis at the current value), letting the user stop on a
 * specific message instead of overshooting whole lanes. v2 used `0.15`
 * (≈6 %/notch), which dogfooding showed was too coarse for precision landing.
 * The {@link MARK_SNAP_FRACTION_THRESHOLD} snap below covers the remaining
 * "land exactly on a mark" gap when smooth scrubbing alone is not enough.
 */
export const WHEEL_SCRUB_PX_PER_DELTA = 0.03;

/**
 * Fractional distance (in the same 0..1 unit the marks use) within which the
 * playhead snaps onto the nearest mark after a wheel scrub. Smooth motion
 * still works — the user can scrub past a mark and the snap then re-engages
 * for the next mark — but landing precisely on a target message becomes
 * effortless. Picked small enough that the snap is invisible at the speeds a
 * user actually scrubs, and zero ⇒ disabled in tests that need exact x.
 *
 * The snap is intentionally NOT applied to click jumps: a click is already an
 * explicit "I meant this exact x" gesture and the dot-distance lookup that
 * picks the active message already maps clicks to a real mark.
 */
export const MARK_SNAP_FRACTION_THRESHOLD = 0.012;

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
 * Expanded/collapsed state for the timeline footer, persisted to localStorage
 * so the preference survives reloads. Initial state is collapsed when no
 * preference has been saved. Exported so tests can drive the toggle directly.
 */
export function useTimelineExpanded(): [boolean, () => void] {
  const [expanded, setExpanded] = useState<boolean>(() => readPersistedExpanded());
  const toggle = useCallback(() => {
    setExpanded((prev) => {
      const next = !prev;
      writePersistedExpanded(next);
      return next;
    });
  }, []);
  return [expanded, toggle];
}

/**
 * Scroll the matching transcript message into view, centred. Scoped to the
 * given container so a duplicate `data-message-uuid` outside the transcript
 * (e.g. in a portaled preview) cannot misdirect the jump.
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
  const target = container.querySelector(
    `[data-message-uuid="${CSS.escape(uuid)}"]`,
  );
  if (target && typeof target.scrollIntoView === 'function') {
    target.scrollIntoView({ block: 'center' });
  }
}

/**
 * Schedule {@link scrollMessageIntoView} to run after the next paint, so a
 * preceding active-thread switch has time to render the target thread's
 * messages into the DOM. Prefers `requestAnimationFrame`; falls back to a
 * zero-delay `setTimeout` when rAF is unavailable (jsdom in vitest does not
 * implement it natively).
 *
 * Returns a cancel handle the caller can fire to suppress the scroll if the
 * component unmounts or another jump supersedes this one before paint.
 */
export function scheduleScrollAfterRender(
  container: HTMLElement | null,
  uuid: string,
): () => void {
  if (
    typeof window !== 'undefined' &&
    typeof window.requestAnimationFrame === 'function'
  ) {
    const handle = window.requestAnimationFrame(() => {
      scrollMessageIntoView(container, uuid);
    });
    return () => window.cancelAnimationFrame(handle);
  }
  const handle = setTimeout(() => {
    scrollMessageIntoView(container, uuid);
  }, 0);
  return () => clearTimeout(handle);
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
 * If the smoothed playhead x is within {@link MARK_SNAP_FRACTION_THRESHOLD}
 * of any mark across every lane, snap it onto that mark's x. Otherwise
 * return the raw x unchanged.
 *
 * Exported so a test can assert the snap behaviour without driving the
 * full wheel-event loop. The threshold of 0 disables the snap (callers that
 * want exact-x scrubbing for measurement can set the constant to 0).
 */
export function snapToNearestMark(
  rawX: number,
  lanes: { dots: { x: number }[] }[],
  threshold: number = MARK_SNAP_FRACTION_THRESHOLD,
): number {
  if (threshold <= 0) {
    return rawX;
  }
  let nearestX = rawX;
  let nearestDistance = threshold;
  for (const lane of lanes) {
    for (const dot of lane.dots) {
      const distance = Math.abs(dot.x - rawX);
      if (distance <= nearestDistance) {
        nearestDistance = distance;
        nearestX = dot.x;
      }
    }
  }
  return nearestX;
}

/**
 * Mark width / height in pixels.
 *
 * v3 switched from a circle to a thin vertical rectangle so a packed lane stays
 * readable: rectangles don't overlap-blur the way circles do at high density,
 * and the height makes the role color (user vs other) easy to scan along the
 * lane. The width is small enough to land on a single mark when the wheel
 * snap engages, and the height fills most of the lane row.
 */
const MARK_WIDTH_PX = 3;
const MARK_HEIGHT_PX = 12;
/** Width reserved on the left for lane labels. */
const LABEL_COLUMN_PX = 88;
/** Width reserved for the right-hand padding inside the lane area. */
const LANE_RIGHT_PAD_PX = 16;

/**
 * The fixed footer between the conversation pane and the composer: a swim-lane
 * timeline of every subthread (and the main thread). Each thread is a row,
 * each speech turn is a dot, and every dot sits on a SHARED time axis driven
 * by the message's `created_at` — idle and thinking gaps render as horizontal
 * whitespace, so the time order is visible at a glance rather than being
 * flattened into equal speech-order spacing.
 *
 * A vertical playhead spans every lane and is the user's scrub handle:
 * scrolling the mouse wheel over the footer moves it left/right (and the
 * default vertical scroll is suppressed while the wheel is over the footer),
 * and clicking the timeline jumps it to that x. Whichever message dot's x is
 * closest to the playhead becomes "active": its lane is highlighted, the
 * active thread switches to that lane (firing the existing nav setter), and
 * after the next paint the matching message is scrolled into view in the
 * conversation pane. Hovering a dot only changes the cursor — there is no
 * hover-driven navigation; the playhead is the single source of truth.
 *
 * The footer is always present; clicking the title bar collapses or expands
 * the lanes, and the preference is persisted per device.
 *
 * Marks are color-coded by author (user vs everything else); cross-row
 * derivation lines, playhead drag, zoom, click-to-pin, and finer-grained
 * "other" coloring (assistant vs tool vs meta) are tracked as separate
 * follow-ups.
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

  // The lane axis is fixed-width — horizontal scroll is acceptable for very
  // long sessions in MVP. Keeping it bounded means dot positions are
  // deterministic from the data alone (no parent-width measurement), which
  // simplifies the click-to-jump and wheel-scrub math considerably.
  const laneAxisWidth = MIN_LANE_AXIS_PX;

  // Playhead position in the same 0..1 unit dots use. Initial position is the
  // latest dot's x (so a freshly-opened session lands on the most recent
  // utterance), falling back to 1 (the right edge) when no dot exists yet.
  const latestDotX = useMemo(() => {
    let max = -Infinity;
    for (const lane of lanes) {
      for (const dot of lane.dots) {
        if (dot.x > max) {
          max = dot.x;
        }
      }
    }
    return Number.isFinite(max) ? max : 1;
  }, [lanes]);

  const [playheadX, setPlayheadXState] = useState<number>(latestDotX);
  // A monotonically-increasing counter incremented on every user-driven
  // playhead move. The thread-switch + scroll effect below uses it as a
  // re-trigger so a re-click at the playhead's current position (and thus the
  // current active message) still re-fires the jump — without it React would
  // bail out of the state set when the value is unchanged, swallowing the
  // user's intent. The counter ALSO doubles as the "has the user scrubbed?"
  // gate: while it sits at 0, the effect is intentionally inert so an
  // automatic mount settle never hijacks the user's chosen thread.
  const [scrubTick, setScrubTick] = useState(0);
  const setPlayheadX = useCallback((next: number) => {
    setScrubTick((tick) => tick + 1);
    setPlayheadXState(next);
  }, []);

  // Re-anchor to the latest dot whenever a new message lands at a brand-new
  // axis extreme — but only while the user has not yet scrubbed. A scrub
  // pins the playhead to whatever value the user picked, and moving it off
  // that point on a fresh message would feel like the timeline yanked away.
  // Use a functional setter so we can compare against the live playhead
  // without listing it as a dep (which would re-run this effect every time
  // the user scrubs, just to confirm there is nothing to do).
  useEffect(() => {
    if (scrubTick !== 0) {
      return;
    }
    setPlayheadXState((prev) => (prev === latestDotX ? prev : latestDotX));
  }, [latestDotX, scrubTick]);

  const activeMatch = useMemo(
    () => findActiveMessage(lanes, playheadX),
    [lanes, playheadX],
  );

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

  useEffect(() => {
    // Only react to scrubs the user actually initiated: while `scrubTick`
    // sits at its initial 0 (a fresh mount with no click or wheel yet), the
    // automatic settle must not flip the active thread the user chose
    // elsewhere.
    if (scrubTick === 0) {
      return;
    }
    if (activeMatch === null) {
      return;
    }
    pendingScrollCancelRef.current?.();
    pendingScrollCancelRef.current = null;
    const container = conversationBodyRef.current;
    if (activeMatch.threadId === activeThreadRef.current) {
      // Same lane: the target message is already in the DOM, scroll right
      // away. No frame deferral, no thread switch.
      scrollMessageIntoView(container, activeMatch.uuid);
      return;
    }
    // Cross-lane jump: switch the active thread first so the conversation
    // pane re-renders with the target lane's messages, then scroll on the
    // next frame once those nodes have landed in the DOM.
    setActiveThread(activeMatch.threadId);
    pendingScrollCancelRef.current = scheduleScrollAfterRender(
      container,
      activeMatch.uuid,
    );
    // `scrubTick` is the re-trigger: a re-click at the playhead's current x
    // (yielding the same activeMatch) bumps the tick and re-fires this
    // effect, so a stale conversation-body scroll position is corrected even
    // when the playhead did not move.
  }, [scrubTick, activeMatch, conversationBodyRef, setActiveThread]);

  // The lane the highlight follows: the playhead-active lane when there is
  // one, else fall back to the prop so a freshly-mounted footer still marks
  // the active thread before any dots have rendered.
  const highlightedThreadId = activeMatch?.threadId ?? activeThreadId;

  // The scrubbable area is the union of every lane's axis: a wheel over any
  // part of the footer body moves the playhead. The body's onWheel is the
  // entry point — its `passive: false` listener (registered via the effect
  // below) is required to call `preventDefault` and suppress the page scroll.
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const playheadXRef = useRef(playheadX);
  useEffect(() => {
    playheadXRef.current = playheadX;
  }, [playheadX]);

  useEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    const onWheel = (event: WheelEvent) => {
      // Suppress the page's vertical scroll while the wheel is over the
      // footer: the wheel belongs to the playhead while it sits here.
      event.preventDefault();
      // `deltaX` from horizontal trackpad scrolls is honoured too — the user
      // gets to pick whichever axis their device emits. Sum so a diagonal
      // gesture (rare but possible) reads as the combined intent.
      const rawDelta = event.deltaY + event.deltaX;
      if (rawDelta === 0) {
        return;
      }
      const px = rawDelta * WHEEL_SCRUB_PX_PER_DELTA;
      // Map the pixel delta to a fractional delta on the same axis the marks
      // use, then clamp into 0..1 so the playhead never strays off the axis.
      const fractionDelta = px / laneAxisWidth;
      const raw = Math.max(
        0,
        Math.min(1, playheadXRef.current + fractionDelta),
      );
      // Apply snap-to-nearest-mark: if the smoothed playhead position is
      // within MARK_SNAP_FRACTION_THRESHOLD of any mark, pull it onto that
      // mark's x. The threshold is small enough that continued wheel motion
      // pushes the next step past the snap radius, so the playhead escapes
      // toward the next mark instead of getting stuck — smooth scrubbing
      // and precise landing coexist.
      const snapped = snapToNearestMark(raw, lanes);
      if (snapped !== playheadXRef.current) {
        setPlayheadX(snapped);
      }
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [laneAxisWidth, expanded, lanes]);

  // Click anywhere on the timeline body jumps the playhead to that x.
  const handleClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      // The click target is the lane-body div; the playhead row inside any
      // lane is what carries the axis the playhead aligns against. Use the
      // body's bounding rect minus the label column so the conversion mirrors
      // the absolute positioning of the playhead line itself.
      const body = bodyRef.current;
      if (!body) {
        return;
      }
      // Locate the first lane row (every lane shares the same axis width and
      // x-origin) so the click → x conversion uses the axis the dots actually
      // sit on. Falling back to the body's own rect would include the label
      // column and right padding, throwing the conversion off.
      const axisEl = body.querySelector<HTMLElement>('[data-timeline-axis]');
      if (!axisEl) {
        return;
      }
      const rect = axisEl.getBoundingClientRect();
      // The axis row's pixel width includes the right padding the dots do not
      // use; map against the dot-bearing width (`laneAxisWidth`) so a click
      // exactly on a dot lands the playhead on it, not slightly off.
      if (laneAxisWidth <= 0) {
        return;
      }
      const fraction = (event.clientX - rect.left) / laneAxisWidth;
      const clamped = Math.max(0, Math.min(1, fraction));
      setPlayheadX(clamped);
    },
    [laneAxisWidth],
  );

  return (
    <section
      data-testid="thread-timeline-overlay"
      data-expanded={expanded ? 'true' : 'false'}
      className="select-none rounded-md border border-slate-200 bg-white text-xs text-slate-600 shadow-sm"
      aria-label="Subthread timeline"
    >
      <button
        type="button"
        onClick={toggle}
        data-testid="thread-timeline-toggle"
        aria-expanded={expanded}
        className="flex w-full items-center justify-between gap-2 rounded-md px-2 py-1 text-left font-medium text-slate-500 transition-colors hover:bg-slate-50"
      >
        <span className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className={`inline-block h-1.5 w-1.5 rounded-full ${
              expanded ? 'bg-slate-500' : 'bg-slate-300'
            }`}
          />
          Thread timeline
          {lanes.length > 0 && (
            <span className="text-slate-400">({lanes.length})</span>
          )}
        </span>
        <span aria-hidden="true" className="text-slate-400">
          {expanded ? '▾' : '▸'}
        </span>
      </button>
      {expanded && (
        <div
          ref={bodyRef}
          data-testid="thread-timeline-body"
          className="max-h-40 overflow-auto px-2 pb-1"
          onClick={handleClick}
        >
          {lanes.length === 0 ? (
            <p className="px-1 py-1 text-[0.7rem] text-slate-400">
              No threads to show yet.
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5" role="list">
              {lanes.map((lane) => {
                const isHighlighted = lane.threadId === highlightedThreadId;
                return (
                  <li
                    key={lane.threadId}
                    data-testid="thread-timeline-lane"
                    data-thread-id={lane.threadId}
                    data-active={isHighlighted ? 'true' : 'false'}
                    className={`flex items-center gap-2 rounded-sm px-1 ${
                      isHighlighted
                        ? 'border-y border-slate-200 bg-slate-50'
                        : 'border-y border-transparent'
                    }`}
                    style={{ minHeight: LANE_HEIGHT_PX }}
                  >
                    <span
                      title={lane.tooltip}
                      data-testid="thread-timeline-lane-label"
                      className={`block shrink-0 truncate font-mono text-[0.65rem] ${
                        lane.isMain ? 'text-slate-700' : 'text-slate-500'
                      }`}
                      style={{ width: LABEL_COLUMN_PX }}
                    >
                      {lane.label}
                    </span>
                    <div
                      data-timeline-axis=""
                      className="relative shrink-0"
                      style={{
                        width: laneAxisWidth + LANE_RIGHT_PAD_PX,
                        height: LANE_HEIGHT_PX,
                      }}
                    >
                      <span
                        aria-hidden="true"
                        className="absolute left-0 top-1/2 h-px -translate-y-1/2 bg-slate-200"
                        style={{ width: laneAxisWidth }}
                      />
                      {lane.dots.map((dot) => (
                        <TimelineDotMark
                          key={dot.uuid}
                          dot={dot}
                          laneAxisWidth={laneAxisWidth}
                        />
                      ))}
                      {/* Playhead: a thin vertical line that doubles as the
                          lane-local segment of the global playhead. Each lane
                          carries its own copy (instead of one absolute line
                          spanning the body) so it scrolls with the lanes when
                          the body becomes scrollable on a long session. */}
                      <span
                        aria-hidden="true"
                        data-testid="thread-timeline-playhead"
                        className="pointer-events-none absolute top-0 h-full w-px bg-indigo-500"
                        style={{ left: playheadX * laneAxisWidth }}
                      />
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

interface TimelineDotMarkProps {
  dot: TimelineDot;
  laneAxisWidth: number;
}

/**
 * One mark within a lane. v3 renders it as a thin vertical rectangle (was a
 * circle in earlier iterations) so a packed lane stays readable, and colors
 * it by author kind — user turns in blue, everything else in slate — so the
 * shape of the conversation is visible at a glance. The tokens mirror the
 * transcript bubble palette (`bg-blue-*` for user, `bg-slate-*` for assistant
 * /tool/etc.) so the timeline reads as the same conversation, just compressed.
 *
 * The mark is non-interactive: hover and click navigation flow through the
 * playhead alone, so a mark is purely a visual anchor (and the wheel-snap's
 * target).
 */
function TimelineDotMark({ dot, laneAxisWidth }: TimelineDotMarkProps) {
  const left = dot.x * laneAxisWidth;
  // Two-color scheme: user vs everything else. Mirrors `MessageItem`'s
  // bubble palette family — blue for user, slate for the assistant side —
  // but at a saturation that reads well at 3 × 12 px. Finer-grained
  // distinction within "other" is deferred to a follow-up.
  const colorClass =
    dot.kind === 'user'
      ? 'bg-blue-500'
      : 'bg-slate-400';
  return (
    <span
      data-testid="thread-timeline-dot"
      data-message-uuid={dot.uuid}
      data-thread-id={dot.threadId}
      data-message-kind={dot.kind}
      aria-hidden="true"
      className={`pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-sm ${colorClass}`}
      style={{
        left,
        width: MARK_WIDTH_PX,
        height: MARK_HEIGHT_PX,
      }}
    />
  );
}
