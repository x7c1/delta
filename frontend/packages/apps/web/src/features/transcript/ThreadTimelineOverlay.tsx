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
  buildSortedMessages,
  buildTimelineLanes,
  findNearestMessageIndex,
  type TimelineDot,
} from './timelineLanes';

/**
 * localStorage key for the timeline footer's expanded/collapsed state. Per
 * device, not per session — the user's preference travels across sessions.
 */
export const TIMELINE_EXPANDED_STORAGE_KEY = 'delta.thread-timeline-overlay.expanded';

/**
 * Minimum gap (ms) between two wheel notches that the navigation accepts as
 * distinct steps. Trackpads (and high-resolution mice) fan a single deliberate
 * gesture into many small `deltaY` events; without a cooldown the active index
 * would skip several messages on one flick. Tuned to ~120 ms so a confident
 * deliberate flick still produces one step, while continuous inertial spam
 * collapses into one step per cooldown window.
 *
 * Exported so a test can drive the cooldown explicitly without having to
 * wait wall-clock time between dispatches.
 */
export const WHEEL_COOLDOWN_MS = 120;

/**
 * CSS transition duration (ms) for the playhead's `left` animation. Short
 * enough that the user always feels the playhead is "tracking" their input,
 * long enough that the discrete step between adjacent messages does not
 * teleport jarringly.
 */
const PLAYHEAD_TRANSITION_MS = 100;

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
 * Mark width / height in pixels.
 *
 * Thin vertical rectangles so a packed lane stays readable: rectangles do not
 * overlap-blur the way circles do at high density, and the height makes the
 * role color (user vs other) easy to scan along the lane.
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
 * each speech turn is a mark, and every mark sits on a SHARED time axis driven
 * by the message's `created_at` — idle and thinking gaps render as horizontal
 * whitespace, so the time order is visible at a glance rather than being
 * flattened into equal speech-order spacing.
 *
 * The vertical playhead is a follower: it tracks the active message's x and
 * is never freely draggable. Wheel events advance discretely through the
 * SINGLE timeline-sorted list of every (sub)thread's messages — one notch
 * jumps to the next-or-previous message in global time order, including
 * across lanes, with cooldown debouncing so a trackpad's inertial fan-out
 * still reads as one deliberate step. Clicking anywhere on the timeline
 * jumps the active index to the message whose x is closest to the click.
 * Whichever message is active becomes the lane highlight, fires the existing
 * nav setter, and after the next paint is scrolled into view in the
 * conversation pane. Hovering a mark does nothing — the playhead is the
 * single source of truth.
 *
 * The footer is always present; clicking the title bar collapses or expands
 * the lanes, and the preference is persisted per device.
 *
 * Marks are color-coded by author (user vs everything else); cross-row
 * derivation lines, zoom, and finer-grained "other" coloring (assistant vs
 * tool vs meta) are tracked as separate follow-ups.
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

  // The global sorted list of every message across every lane, ordered by
  // (created_at asc, seq asc). The active index navigates this list directly,
  // so a single wheel notch always advances by exactly one message — sparse
  // and dense lanes alike step by 1, with no fractional-x math to overshoot.
  const sortedMessages = useMemo(() => buildSortedMessages(lanes), [lanes]);

  // The lane axis is fixed-width — horizontal scroll is acceptable for very
  // long sessions in MVP. Keeping it bounded means dot positions are
  // deterministic from the data alone (no parent-width measurement), which
  // simplifies the click-to-jump math considerably.
  const laneAxisWidth = MIN_LANE_AXIS_PX;

  // The active message's index in `sortedMessages`. A fresh mount lands on
  // the latest message so a newly opened session highlights the most recent
  // utterance. `null` means there are no messages to land on yet.
  const [activeMessageIndex, setActiveMessageIndexState] = useState<number | null>(
    () => (sortedMessages.length === 0 ? null : sortedMessages.length - 1),
  );

  // A monotonically-increasing counter incremented on every user-driven
  // navigation. The thread-switch + scroll effect below uses it as a
  // re-trigger so a re-click at the playhead's current position (and thus
  // the current active index) still re-fires the jump — without it React
  // would bail out of the state set when the value is unchanged, swallowing
  // the user's intent. The counter ALSO doubles as the "has the user
  // navigated?" gate: while it sits at 0, the effect is intentionally inert
  // so an automatic mount settle never hijacks the user's chosen thread.
  const [scrubTick, setScrubTick] = useState(0);

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
      setActiveMessageIndexState(clamped);
    },
    [sortedMessages.length],
  );

  // Re-anchor to the latest message whenever a new one lands at the tail
  // while the user has not yet navigated. A navigation pins the active index
  // to whatever the user picked, and moving it on a fresh message would feel
  // like the timeline yanked away.
  useEffect(() => {
    if (scrubTick !== 0) {
      return;
    }
    setActiveMessageIndexState((prev) => {
      if (sortedMessages.length === 0) {
        return null;
      }
      const latest = sortedMessages.length - 1;
      return prev === latest ? prev : latest;
    });
  }, [sortedMessages, scrubTick]);

  // The active message itself, derived from the index — the single source of
  // truth for the playhead's x. There is no separately-stored fractional
  // position state: keeping the playhead a pure function of the active
  // message means continuous wheel inertia can never desync from a discrete
  // step.
  const activeMessage =
    activeMessageIndex !== null && activeMessageIndex < sortedMessages.length
      ? sortedMessages[activeMessageIndex]
      : null;
  const playheadX = activeMessage?.x ?? 0;

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
    // Only react to navigation the user actually initiated: while
    // `scrubTick` sits at its initial 0 (a fresh mount with no wheel or
    // click yet), the automatic settle must not flip the active thread the
    // user chose elsewhere.
    if (scrubTick === 0) {
      return;
    }
    if (activeMessage === null) {
      return;
    }
    pendingScrollCancelRef.current?.();
    pendingScrollCancelRef.current = null;
    const container = conversationBodyRef.current;
    if (activeMessage.threadId === activeThreadRef.current) {
      // Same lane: the target message is already in the DOM, scroll right
      // away. No frame deferral, no thread switch.
      scrollMessageIntoView(container, activeMessage.uuid);
      return;
    }
    // Cross-lane jump: switch the active thread first so the conversation
    // pane re-renders with the target lane's messages, then scroll on the
    // next frame once those nodes have landed in the DOM.
    setActiveThread(activeMessage.threadId);
    pendingScrollCancelRef.current = scheduleScrollAfterRender(
      container,
      activeMessage.uuid,
    );
    // `scrubTick` is the re-trigger: a re-click at the same x (yielding the
    // same activeMessage) bumps the tick and re-fires this effect, so a
    // stale conversation-body scroll position is corrected even when the
    // active index did not move.
  }, [scrubTick, activeMessage, conversationBodyRef, setActiveThread]);

  // The lane the highlight follows: the active message's lane when there is
  // one, else fall back to the prop so a freshly-mounted footer still marks
  // the active thread before any messages have loaded.
  const highlightedThreadId = activeMessage?.threadId ?? activeThreadId;

  // The scrubbable area is the union of every lane's axis: a wheel over any
  // part of the footer body advances the active index. The body's onWheel is
  // the entry point — its `passive: false` listener (registered via the
  // effect below) is required to call `preventDefault` and suppress the page
  // scroll.
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const activeMessageIndexRef = useRef(activeMessageIndex);
  useEffect(() => {
    activeMessageIndexRef.current = activeMessageIndex;
  }, [activeMessageIndex]);
  const sortedMessagesRef = useRef(sortedMessages);
  useEffect(() => {
    sortedMessagesRef.current = sortedMessages;
  }, [sortedMessages]);

  // Last-accepted wheel timestamp for the cooldown debounce. A trackpad's
  // inertial fan-out fires many small `deltaY` events for one deliberate
  // gesture; the cooldown collapses that burst into a single step so the
  // user cannot accidentally skip multiple messages.
  const lastWheelMsRef = useRef(0);

  useEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    const onWheel = (event: WheelEvent) => {
      // Suppress the page's vertical scroll while the wheel is over the
      // footer: the wheel belongs to the active-index step while it sits
      // here.
      event.preventDefault();
      // `deltaX` from horizontal trackpad scrolls is honoured too — the
      // user gets to pick whichever axis their device emits. Sum so a
      // diagonal gesture (rare but possible) reads as the combined intent.
      const rawDelta = event.deltaY + event.deltaX;
      if (rawDelta === 0) {
        return;
      }
      const now =
        typeof performance !== 'undefined' &&
        typeof performance.now === 'function'
          ? performance.now()
          : Date.now();
      if (now - lastWheelMsRef.current < WHEEL_COOLDOWN_MS) {
        return;
      }
      lastWheelMsRef.current = now;
      const total = sortedMessagesRef.current.length;
      if (total === 0) {
        return;
      }
      const currentIndex = activeMessageIndexRef.current ?? total - 1;
      // Wheel down (positive delta) → next message (newer); wheel up →
      // previous (older). Clamped to the ends — no wrap.
      const step = rawDelta > 0 ? 1 : -1;
      const next = Math.max(0, Math.min(total - 1, currentIndex + step));
      if (next === currentIndex) {
        return;
      }
      setActiveMessageIndex(next);
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [expanded, setActiveMessageIndex]);

  // Click anywhere on the timeline body jumps the active index to the
  // message whose x is closest to the click. There is no distance threshold:
  // a click anywhere in the overlay accepts the global nearest message —
  // the overlay is small enough that the closest mark is always the user's
  // intent, and a threshold would otherwise swallow clicks on the empty
  // axis whitespace between marks.
  const handleClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      const body = bodyRef.current;
      if (!body) {
        return;
      }
      // Locate the first lane row (every lane shares the same axis width
      // and x-origin) so the click → x conversion uses the axis the dots
      // actually sit on. Falling back to the body's own rect would include
      // the label column and right padding, throwing the conversion off.
      const axisEl = body.querySelector<HTMLElement>('[data-timeline-axis]');
      if (!axisEl) {
        return;
      }
      const rect = axisEl.getBoundingClientRect();
      if (laneAxisWidth <= 0) {
        return;
      }
      const fraction = (event.clientX - rect.left) / laneAxisWidth;
      const clamped = Math.max(0, Math.min(1, fraction));
      const nearest = findNearestMessageIndex(sortedMessages, clamped);
      if (nearest < 0) {
        return;
      }
      setActiveMessageIndex(nearest);
    },
    [laneAxisWidth, sortedMessages, setActiveMessageIndex],
  );

  // After a user-driven navigation, ensure the playhead is visible inside
  // the body's horizontal viewport. The lane axis is fixed-width so this
  // only matters when the viewport is narrower than the axis (e.g. a narrow
  // side panel); when the axis fits, no scroll is needed.
  useEffect(() => {
    if (scrubTick === 0) {
      return;
    }
    const body = bodyRef.current;
    if (!body || activeMessage === null) {
      return;
    }
    const axisEl = body.querySelector<HTMLElement>('[data-timeline-axis]');
    if (!axisEl) {
      return;
    }
    // The playhead's offset within the scrollable body — its left position
    // inside the axis plus the axis's offset from the body (lane label).
    const playheadInAxis = activeMessage.x * laneAxisWidth;
    const playheadInBody = axisEl.offsetLeft + playheadInAxis;
    const viewLeft = body.scrollLeft;
    const viewRight = viewLeft + body.clientWidth;
    if (playheadInBody < viewLeft || playheadInBody > viewRight) {
      body.scrollLeft = Math.max(0, playheadInBody - body.clientWidth / 2);
    }
  }, [scrubTick, activeMessage, laneAxisWidth]);

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
                          lane-local segment of the global playhead. Each
                          lane carries its own copy (instead of one absolute
                          line spanning the body) so it scrolls with the
                          lanes when the body becomes scrollable on a long
                          session. A short CSS transition on `left` smooths
                          the discrete step between adjacent messages so the
                          eye can follow the move; the duration is short
                          enough that the playhead always feels glued to the
                          input. */}
                      <span
                        aria-hidden="true"
                        data-testid="thread-timeline-playhead"
                        className="pointer-events-none absolute top-0 h-full w-px bg-indigo-500"
                        style={{
                          left: playheadX * laneAxisWidth,
                          transition: `left ${PLAYHEAD_TRANSITION_MS}ms ease-out`,
                        }}
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
 * One mark within a lane. Rendered as a thin vertical rectangle so a packed
 * lane stays readable, colored by author kind — user turns in blue,
 * everything else in slate — so the shape of the conversation is visible at
 * a glance. The tokens mirror the transcript bubble palette (`bg-blue-*` for
 * user, `bg-slate-*` for assistant/tool/etc.) so the timeline reads as the
 * same conversation, just compressed.
 *
 * The mark is non-interactive: hover and click navigation flow through the
 * playhead alone, so a mark is purely a visual anchor.
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
