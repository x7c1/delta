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
 * Scroll the matching transcript message into view, aligned to the top of
 * the scrollable body. Scoped to the given container so a duplicate
 * `data-message-uuid` outside the transcript (e.g. in a portaled preview)
 * cannot misdirect the jump.
 *
 * Using `block: 'start'` rather than the v6 `block: 'center'` means the
 * destination message becomes the first line the eye reads on the next
 * paint — a centred message wastes half the viewport above the line the
 * user just asked to jump to. The transcript body's floating breadcrumb
 * overlay would otherwise hide the top of the article; that is compensated
 * by a global `scroll-margin-top` rule on `article[data-message-uuid]` (see
 * index.css) so the article scrolls to just below the breadcrumb.
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
    target.scrollIntoView({ block: 'start' });
  }
}

/**
 * Briefly mark the matching transcript message with the jump-highlight class
 * so the eye spots where the navigation landed. The class sets a temporary
 * background-color on the bubble and the CSS transition fades it back to the
 * resting color — no overlay layer, the highlight lands directly on the
 * message body. Scoped to the given container for the same reason as
 * {@link scrollMessageIntoView}: a duplicate `data-message-uuid` outside
 * the transcript (e.g. in a portaled preview) must not steal the highlight.
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
  const target = container.querySelector(
    `[data-message-uuid="${CSS.escape(uuid)}"]`,
  );
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
  let highlightCancel: (() => void) | null = null;
  const run = () => {
    scrollMessageIntoView(container, uuid);
    highlightCancel = highlightMessageJump(container, uuid);
  };
  if (
    typeof window !== 'undefined' &&
    typeof window.requestAnimationFrame === 'function'
  ) {
    const handle = window.requestAnimationFrame(run);
    return () => {
      window.cancelAnimationFrame(handle);
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
const MARK_LARGE_PX = 6;
const MARK_SMALL_PX = 4;
/**
 * Diameter (px) of a cluster mark — slightly larger than a lone small dot so
 * the eye distinguishes the two: a single small dot is one tool call, a
 * cluster is "several here in a row". Kept below {@link MARK_LARGE_PX} so a
 * cluster still reads as auxiliary chatter, not a headline turn.
 */
const MARK_CLUSTER_PX = 5;
/** Width reserved on the left for lane labels. */
const LABEL_COLUMN_PX = 88;
/** Width reserved for the right-hand padding inside the lane area. */
const LANE_RIGHT_PAD_PX = 16;

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
 * skipping the surrounding tool calls, meta lines, and question cards. The
 * cooldown debounce keeps a trackpad's inertial fan-out reading as one
 * deliberate step. Clicking anywhere on the timeline jumps the active index
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

  // Keep the active index pointing at the SAME message across a
  // `sortedMessages` reference change (e.g. a background refetch landed a
  // new array with the same content, or a fresh message appended at the
  // tail). Without this the index would drift relative to the message the
  // user picked, and the wheel/click handlers would step from the wrong
  // anchor. A `null` index (no messages yet, or the picked message vanished)
  // falls back to the latest entry.
  useEffect(() => {
    if (scrubTick === 0) {
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
  }, [sortedMessages, scrubTick]);

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
      scrollMessageIntoView(container, current.uuid);
      pendingScrollCancelRef.current = highlightMessageJump(
        container,
        current.uuid,
      );
      return;
    }
    // Cross-lane jump: switch the active thread first so the conversation
    // pane re-renders with the target lane's messages, then scroll + flash
    // on the next frame once those nodes have landed in the DOM.
    setActiveThread(current.threadId);
    pendingScrollCancelRef.current = scheduleScrollAfterRender(
      container,
      current.uuid,
    );
    // `scrubTick` is the re-trigger AND the gate: a fresh scrub bumps the
    // tick, re-fires this effect, and re-emits the (possibly identical)
    // navigation intent. A re-click at the same x bumps the tick even when
    // the active index does not move, so a stale scroll position is still
    // corrected — but a tick-less re-render never sneaks in an auto-switch
    // that the user did not ask for.
  }, [scrubTick, conversationBodyRef, setActiveThread]);

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
  const largeSortedMessagesRef = useRef(largeSortedMessages);
  useEffect(() => {
    largeSortedMessagesRef.current = largeSortedMessages;
  }, [largeSortedMessages]);

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
      const large = largeSortedMessagesRef.current;
      if (total === 0 || large.length === 0) {
        return;
      }
      // Wheel down (positive delta) → next message (newer); wheel up →
      // previous (older). Clamped to the ends — no wrap.
      const step = rawDelta > 0 ? 1 : -1;
      const currentIndex = activeMessageIndexRef.current ?? total - 1;
      const currentMessage = sortedMessagesRef.current[currentIndex];
      // Find the next/previous LARGE message relative to where the playhead
      // currently sits. When the playhead is on a large mark, that mark is in
      // `large` itself — pick the neighbour at `largeIdx + step`. When it is
      // on a small mark (a click jumped to a tool call), pick the nearest
      // large neighbour in the requested direction so the very first wheel
      // notch still produces a visible step rather than a no-op.
      const nextLarge = pickNeighbourLargeMessage(large, currentMessage, step);
      if (nextLarge === null) {
        return;
      }
      const nextGlobalIndex = sortedMessagesRef.current.findIndex(
        (m) => m.uuid === nextLarge.uuid,
      );
      if (nextGlobalIndex < 0 || nextGlobalIndex === currentIndex) {
        return;
      }
      setActiveMessageIndex(nextGlobalIndex);
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
      // Translate the click to the same absolute-px space the global x map
      // uses, clamped to [0, axisWidth] so a click in the right-hand padding
      // still snaps to the rightmost mark.
      const offsetPx = event.clientX - rect.left;
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
    // inside the axis (from the global x map) plus the axis's offset from
    // the body (lane label).
    const playheadInAxis = messagePxByUuid.get(activeMessage.uuid) ?? 0;
    const playheadInBody = axisEl.offsetLeft + playheadInAxis;
    const viewLeft = body.scrollLeft;
    const viewRight = viewLeft + body.clientWidth;
    if (playheadInBody < viewLeft || playheadInBody > viewRight) {
      body.scrollLeft = Math.max(0, playheadInBody - body.clientWidth / 2);
    }
  }, [scrubTick, activeMessage, laneAxisWidth, messagePxByUuid]);

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
            // `w-max min-w-full` makes the ul as wide as the widest lane
            // (label + axis + right padding), but never narrower than the
            // body itself. Each `<li>` inside (`flex` items default to
            // `align-self: stretch` on the cross axis of the column-flex
            // parent) then stretches to that same intrinsic width, so the
            // sticky label's containing block — the `<li>` — spans the
            // FULL horizontal scroll range. Without `w-max`, the ul stays
            // at the body's content width; the `<li>` stretches only to
            // body-width while its `shrink-0` children overflow it; the
            // sticky label then hits its `<li>`'s right edge partway
            // through the scroll and gets pinned there, sliding leftward
            // out of view as the body keeps scrolling right. `min-w-full`
            // keeps short sessions (axis narrower than the body) at full
            // width so the layout does not collapse to label-only.
            <ul className="flex w-max min-w-full flex-col gap-0.5" role="list">
              {lanes.map((lane) => {
                const isHighlighted = lane.threadId === highlightedThreadId;
                // Collapse runs of 2+ consecutive small dots within this lane
                // into one cluster mark so a long stretch of tool calls / meta
                // lines no longer floods the timeline. Lone small dots and
                // every large dot still render individually.
                const renderItems = buildLaneRenderItems(lane.dots);
                return (
                  <li
                    key={lane.threadId}
                    data-testid="thread-timeline-lane"
                    data-thread-id={lane.threadId}
                    data-active={isHighlighted ? 'true' : 'false'}
                    // No `gap-2`: the sticky label carries its own right
                    // padding so its background covers right up to the axis
                    // — otherwise dots could show through the gap as the
                    // body scrolls horizontally past the label.
                    className={`flex items-center rounded-sm pr-1 ${
                      isHighlighted
                        ? 'border-y border-slate-200 bg-slate-50'
                        : 'border-y border-transparent'
                    }`}
                    style={{ minHeight: LANE_HEIGHT_PX }}
                  >
                    <span
                      title={lane.tooltip}
                      data-testid="thread-timeline-lane-label"
                      // The label column stays pinned to the left edge of
                      // the body during horizontal scroll (`position: sticky;
                      // left: 0`) so "which lane is which" never scrolls off
                      // screen on a dense session. The background color
                      // matches the lane (active = `bg-slate-50`, otherwise
                      // the body's `bg-white`) so axis dots cannot show
                      // through behind the label; the surrounding `gap-2`
                      // would otherwise leave the label transparent.
                      className={`sticky left-0 z-10 block shrink-0 truncate py-0.5 pl-1 pr-2 font-mono text-[0.65rem] ${
                        lane.isMain ? 'text-slate-700' : 'text-slate-500'
                      } ${isHighlighted ? 'bg-slate-50' : 'bg-white'}`}
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
                      {renderItems.map((item) =>
                        item.kind === 'dot' ? (
                          <TimelineDotMark
                            key={item.dot.uuid}
                            dot={item.dot}
                            xPx={messagePxByUuid.get(item.dot.uuid) ?? 0}
                          />
                        ) : (
                          <TimelineClusterMark
                            key={item.cluster.key}
                            cluster={item.cluster}
                            xPx={
                              messagePxByUuid.get(
                                item.cluster.representativeUuid,
                              ) ?? 0
                            }
                          />
                        ),
                      )}
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
                          left: playheadX,
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
 * The cluster is rendered slightly larger than a lone small dot
 * ({@link MARK_CLUSTER_PX} vs {@link MARK_SMALL_PX}) so the eye can tell the
 * two apart at a glance — "one tool call" versus "a run of N here". The data
 * attributes carry the representative uuid (matching a regular dot's hook
 * surface) and the member count for downstream diagnostics / tests.
 */
function TimelineClusterMark({ cluster, xPx }: TimelineClusterMarkProps) {
  return (
    <span
      data-testid="thread-timeline-cluster"
      data-message-uuid={cluster.representativeUuid}
      data-thread-id={cluster.threadId}
      data-cluster-member-count={cluster.memberCount}
      aria-hidden="true"
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
