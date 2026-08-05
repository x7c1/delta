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
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import {
  buildGlobalXMap,
  buildLaneRenderItems,
  buildLargeSortedMessages,
  buildSortedMessages,
  buildTimelineLanes,
  computeTimeRange,
  findNearestMessageIndex,
  type SortedMessage,
} from './timelineLanes';
import {
  SkipBackIcon,
  SkipForwardIcon,
  ThreadTimelineIcon,
} from './TimelineIcons';
import {
  markDiameterPx,
  TimelineClusterMark,
  TimelineDotMark,
} from './TimelineMarks';
import {
  ALL_ARTICLES_SELECTOR,
  highlightMessageJump,
  nearestRenderedNeighborUuid,
  scheduleScrollAfterRender,
  scrollMessageIntoView,
} from './timelineScroll';
import { useTimelineExpanded } from './useTimelineExpanded';
import {
  useTimelineKeyboardStepNavigation,
  useTimelineWheelStepNavigation,
} from './useTimelineStepNavigation';

/**
 * CSS transition duration (ms) for the playhead's `left` animation. Short
 * enough that the user always feels the playhead is "tracking" their input,
 * long enough that the discrete step between adjacent messages does not
 * teleport jarringly.
 */
const PLAYHEAD_TRANSITION_MS = 100;

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
 * CSS custom property that carries the top-region reserve height (px). The
 * transcript body sets it inline (see `TranscriptPane`) and message articles
 * read it as their `scroll-margin-top` (see index.css), so a
 * `scrollIntoView({ block: 'start' })` parks the target this many pixels below
 * the container's top edge. The pane-scroll follower reads the same value to
 * locate the reading-region start line (see {@link readTopRegionReserve}).
 */
const TOP_REGION_RESERVE_VAR = '--delta-top-region-reserve';

/**
 * Resolve the top-region reserve (px) currently in effect on the transcript
 * scroll container. Reads the same `--delta-top-region-reserve` custom
 * property the CSS `scroll-margin-top` uses, so the follower's reading-region
 * start line matches exactly where a programmatic `scrollIntoView` parks its
 * target. Falls back to 0 when the variable is unset or unparseable (SSR, or
 * jsdom tests that never install the reserve), which degrades the follower to
 * plain topmost-visible selection.
 */
function readTopRegionReserve(container: Element): number {
  if (typeof getComputedStyle !== 'function') {
    return 0;
  }
  const raw = getComputedStyle(container)
    .getPropertyValue(TOP_REGION_RESERVE_VAR)
    .trim();
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : 0;
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
 * Resolve where the playhead should sit while the user has not navigated yet
 * (the "auto-anchor"): the ACTIVE thread's latest large (main-conversation)
 * turn, so a freshly-opened overlay highlights the lane the user is actually
 * in — not whichever lane happens to hold the global tail.
 *
 * - `activeThreadId === null` (no lane to anchor onto): the global tail.
 * - The active lane has no large turn yet (its messages are still loading, or
 *   the lane only carries tool calls): hold `prev` unchanged. The caller
 *   re-resolves when `largeSortedMessages` next changes, so a lane whose
 *   messages land late still gets anchored — without ever flashing another
 *   lane's tail in the meantime.
 *
 * Shared by the `useState` initializer (with `prev = null`) and the
 * auto-anchor effect so mount and follow-up renders agree on the same pick.
 */
function resolveAutoAnchorUuid(
  sortedMessages: SortedMessage[],
  largeSortedMessages: SortedMessage[],
  activeThreadId: ThreadId | null,
  prev: string | null,
): string | null {
  if (sortedMessages.length === 0) {
    return null;
  }
  if (activeThreadId === null) {
    return sortedMessages[sortedMessages.length - 1].uuid;
  }
  for (let i = largeSortedMessages.length - 1; i >= 0; i -= 1) {
    if (largeSortedMessages[i].threadId === activeThreadId) {
      return largeSortedMessages[i].uuid;
    }
  }
  return prev;
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
  const setActiveThreadWithJumpTarget = useNavStore(
    (state) => state.setActiveThreadWithJumpTarget,
  );
  const clearActiveThreadJumpTarget = useNavStore(
    (state) => state.clearActiveThreadJumpTarget,
  );
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

  // The active message's UUID is the canonical playhead state — NOT its index.
  // The index into `sortedMessages` drifts whenever the array is replaced (a
  // background refetch, a message appended to another lane, ...), so pinning
  // the index and "realigning" it back to the same message on every array
  // change is a race against the effect that snapshots the pick: if the
  // realign pass runs before the snapshot catches up, it resolves a STALE
  // message and reverts a just-committed reposition. Storing the UUID and
  // deriving the index per render (see `activeMessageIndex` below) makes an
  // array-identity change unable to move the playhead by construction.
  //
  // A fresh mount anchors to the active thread's latest large turn (see
  // {@link resolveAutoAnchorUuid}) so the overlay opens on the lane the user
  // is in; the auto-anchor effect below keeps refining the pick as messages
  // load. `null` means there is nothing to land on yet.
  const [activeMessageUuid, setActiveMessageUuidState] = useState<string | null>(
    () =>
      resolveAutoAnchorUuid(
        sortedMessages,
        largeSortedMessages,
        activeThreadId,
        null,
      ),
  );

  // The active message's index in `sortedMessages`, DERIVED from the canonical
  // UUID on every render. When the picked message is not in the current list
  // (deleted, or the session compacted) the index is `null` and the playhead
  // simply has nothing to sit on until the auto-anchor / external-thread
  // effects pick a new target.
  const activeMessageIndex = useMemo<number | null>(() => {
    if (activeMessageUuid === null) {
      return null;
    }
    const index = sortedMessages.findIndex((m) => m.uuid === activeMessageUuid);
    return index < 0 ? null : index;
  }, [sortedMessages, activeMessageUuid]);

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
  // The same gate as a ref, written SYNCHRONOUSLY at the instant the user
  // acts. The auto-anchor effect reads this ref rather than the state:
  // `userActedTick` is captured in each render's closure, so an auto-anchor
  // effect that was queued while the tick still read 0 keeps that stale value
  // even if the user acts before it flushes. React defers passive effects to
  // a later task whenever a commit overruns the scheduler's frame budget, so
  // that window is real — a wheel/arrow step landing in it would be committed
  // and then silently reverted by the late anchor. The ref closes the window
  // by construction: whenever the anchor effect actually runs, it sees the
  // latest intent, not the intent of the render it was scheduled from. The
  // state counter stays because the horizontal scroll catch-up effect needs a
  // value that CHANGES per action to re-fire.
  const userActedRef = useRef(false);
  const bumpUserActedTick = useCallback(() => {
    userActedRef.current = true;
    setUserActedTick((t) => t + 1);
  }, []);

  /**
   * Clamp an index into the valid range for the current sorted list and
   * commit it together with a tick bump that re-fires the navigation
   * effect. Centralising the clamp + tick here keeps the wheel and click
   * handlers from duplicating the same boilerplate.
   *
   * The list is read from {@link sortedMessagesRef} — the same list the wheel
   * / keyboard handlers resolved `next` against — rather than from the render
   * closure. Those handlers are native listeners bound in an earlier commit,
   * so a closure-held array would let an index computed in one list space be
   * resolved in another (a wrong message, or a dropped step when the closure
   * still holds the empty pre-load list). Reading the ref also keeps this
   * callback identity-stable, which is what lets those listeners bind once
   * instead of re-binding on every background-refetch array-identity change.
   */
  const setActiveMessageIndex = useCallback(
    (next: number) => {
      const list = sortedMessagesRef.current;
      if (list.length === 0) {
        return;
      }
      const clamped = Math.max(0, Math.min(list.length - 1, next));
      setScrubTick((tick) => tick + 1);
      bumpUserActedTick();
      // Resolve the index to its message's UUID at commit time — the UUID is
      // the canonical state, so a later array-identity change re-derives the
      // index and keeps the playhead on the exact message the user picked.
      setActiveMessageUuidState(list[clamped].uuid);
    },
    [bumpUserActedTick],
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
   *
   * Unlike {@link setActiveMessageIndex} this one resolves the index against
   * the render closure's list, which is correct here: both callers (the
   * external-thread effect and the observer's debounced flush) derive `next`
   * from the very same render's `sortedMessages`, so the closure and the index
   * always share one list space.
   */
  const setActiveMessageIndexFromPaneScroll = useCallback(
    (next: number) => {
      if (sortedMessages.length === 0) {
        return;
      }
      const clamped = Math.max(0, Math.min(sortedMessages.length - 1, next));
      const uuid = sortedMessages[clamped].uuid;
      let changed = false;
      setActiveMessageUuidState((prev) => {
        if (prev === uuid) {
          return prev;
        }
        changed = true;
        return uuid;
      });
      // Bump the "user has acted" gate only when we actually moved the
      // playhead — repeat IO entries for the same topmost message should not
      // keep flipping the auto-anchor gate on every burst.
      if (changed) {
        bumpUserActedTick();
      }
    },
    [sortedMessages, bumpUserActedTick],
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

  // Snapshot the active message into a ref so the timeline → pane navigation
  // effect (which depends on `scrubTick` alone) can read the latest pick
  // without listing `activeMessage` in its deps — a fresh `sortedMessages`
  // reference from a background refetch swaps the `activeMessage` object
  // identity, and depending on it directly would re-fire the auto-switch that
  // once overrode a user's Navigator click. The sync effect sits ABOVE that
  // navigation effect so the ref is fresh by the time it reads.
  const activeMessageRef = useRef(activeMessage);
  useEffect(() => {
    activeMessageRef.current = activeMessage;
  }, [activeMessage]);

  // Auto-anchor the playhead while the user has not yet navigated. This is the
  // one effect that positions the playhead on mount (and keeps it following as
  // messages load / land) BEFORE any wheel/click/pane-scroll/external-thread
  // action. The target resolution lives in {@link resolveAutoAnchorUuid}: the
  // active thread's latest large turn, retrying via the `largeSortedMessages`
  // dep while the lane's messages are still loading, with the global tail as
  // the fallback only when `activeThreadId` is null.
  //
  // Gated on {@link userActedRef} (not {@link scrubTick}) so any user action —
  // wheel/click jump OR pane-scroll follow OR external-thread reposition —
  // pins the pick and switches this effect off. The gate is read from the ref
  // rather than the `userActedTick` state so it is evaluated at FLUSH time:
  // see the ref's declaration for why a render-time read would let this effect
  // revert a step the user took while it was still queued. There is no
  // companion "realign the index across an array-identity change" effect: the
  // canonical-state note above explains why deriving the index from the UUID
  // makes one unnecessary.
  useEffect(() => {
    if (userActedRef.current) {
      return;
    }
    setActiveMessageUuidState((prev) =>
      resolveAutoAnchorUuid(
        sortedMessages,
        largeSortedMessages,
        activeThreadId,
        prev,
      ),
    );
  }, [sortedMessages, largeSortedMessages, activeThreadId]);

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
  // first-observation batch of the new thread is always ignored. Every
  // increment is paired with exactly one decrement, routed through
  // `scheduleScrollAfterRender`'s `onSettled` callback, which fires once
  // whichever way the schedule terminates — the scroll fired (success), the
  // DOM-ready poll timed out, or the cancel handle ran (superseding jump /
  // unmount). The timeout leg matters: a jump whose target uuid never renders
  // (e.g. an axis click resolving to a renders-nothing carrier message) would
  // otherwise poll to the timeout and return without releasing, latching the
  // counter above zero forever and silently killing both navigator-driven
  // repositioning and pane → playhead follow. Decrements are clamped at zero
  // so a duplicate release (cancel handle invoked after onSettled already
  // fired) cannot wrap into a negative count that would leave the guard
  // permanently armed.
  const crossLaneJumpInFlightCountRef = useRef(0);
  const decrementCrossLaneInFlight = useCallback(() => {
    if (crossLaneJumpInFlightCountRef.current > 0) {
      crossLaneJumpInFlightCountRef.current -= 1;
    }
  }, []);

  // Handle for the pending "clear the jump intent" frame callback, so a
  // superseding jump or unmount can cancel it before it fires.
  const jumpIntentClearRafRef = useRef<number | null>(null);
  // Clear the navigation-intent handoff a couple of animation frames AFTER the
  // jump's scroll settled — NOT synchronously in `onSettled`. The intent is
  // what keeps TranscriptPane's scroll listener from re-arming stick while the
  // jump's own programmatic `scrollIntoView` (and its one-frame reflow recall)
  // fire their scroll events. Per the HTML rendering steps, scroll events are
  // dispatched BEFORE `requestAnimationFrame` callbacks, so a double-rAF defer
  // guarantees the intent is still live when BOTH landing scroll events reach
  // the pane — including a near-tail target whose reflow recall re-clamps at
  // the bottom. Only after that does the intent clear, so a genuine later user
  // scroll re-arms stick normally. Falls back to a synchronous clear when rAF
  // is unavailable (older test runners). `expectedUuid` guards against a newer
  // jump's intent being cleared by an earlier jump's settle.
  const scheduleJumpIntentClear = useCallback(
    (expectedUuid: string) => {
      const clear = () => clearActiveThreadJumpTarget(expectedUuid);
      if (
        typeof window === 'undefined' ||
        typeof window.requestAnimationFrame !== 'function'
      ) {
        clear();
        return;
      }
      if (jumpIntentClearRafRef.current !== null) {
        window.cancelAnimationFrame(jumpIntentClearRafRef.current);
      }
      jumpIntentClearRafRef.current = window.requestAnimationFrame(() => {
        jumpIntentClearRafRef.current = window.requestAnimationFrame(() => {
          jumpIntentClearRafRef.current = null;
          clear();
        });
      });
    },
    [clearActiveThreadJumpTarget],
  );
  useEffect(
    () => () => {
      if (
        jumpIntentClearRafRef.current !== null &&
        typeof window !== 'undefined' &&
        typeof window.cancelAnimationFrame === 'function'
      ) {
        window.cancelAnimationFrame(jumpIntentClearRafRef.current);
        jumpIntentClearRafRef.current = null;
      }
    },
    [],
  );
  // The destination lane of the most recent overlay-driven cross-lane jump.
  // When such a jump calls `setActiveThread`, the `activeThreadId` prop
  // changes to this thread and the external-thread effect re-runs; comparing
  // the prop against this ref lets that effect distinguish its own jump
  // echoing back (skip it) from a genuinely new external navigator pick (act
  // on it). It is cleared back to `null` whenever the external-thread effect
  // commits a (non-jump) reposition, so a stale target can never combine with
  // that effect's own pane-scroll guard counter to misfire the echo-skip
  // check.
  const crossLaneJumpTargetThreadRef = useRef<ThreadId | null>(null);

  // When `activeThreadId` is driven by an external setter (Navigator click in
  // the left pane, breadcrumb, etc.) the conversation pane re-renders into
  // the new subthread, but the timeline's playhead is left pointing at the
  // previous lane's message. On a long session whose axis exceeds the
  // viewport width, the playhead's x then sits outside the horizontal scroll
  // window — invisible to the user — even though the lane highlight already
  // moved to the new lane.
  //
  // Move the playhead to the new lane's latest "large" (main-conversation)
  // turn whenever the external active thread changes to a lane that has at
  // least one large message. The commit is routed through
  // {@link setActiveMessageIndexFromPaneScroll} so it does NOT bump
  // {@link scrubTick} — that would re-fire the timeline → pane jump effect
  // (`scheduleScrollAfterRender` + `setActiveThread`) on the freshly-loaded
  // pane, which is pointless work and would steal scroll focus from the user
  // who just clicked a subthread.
  //
  // Bumping {@link userActedTick} (via `setActiveMessageIndexFromPaneScroll`)
  // does, however, trigger the horizontal scroll catch-up effect below — so
  // the playhead's new x is brought into view automatically. The pane-scroll
  // observer is fenced off for a short window via
  // {@link crossLaneJumpInFlightCountRef} so the IO's first-observation batch
  // on the re-rendered pane cannot race and overwrite the deliberate
  // "latest large turn" target the user implicitly asked for.
  const externalThreadInitializedRef = useRef(false);
  const lastObservedActiveThreadIdRef = useRef<ThreadId | null>(activeThreadId);
  useEffect(() => {
    // Skip the very first render — `activeThreadId` arrives as a prop and the
    // auto-anchor effect already lands the playhead on that thread's latest
    // large turn (retrying as the lane's messages load). Only react to
    // subsequent changes here (the user picked a different subthread from
    // somewhere outside the overlay).
    if (!externalThreadInitializedRef.current) {
      externalThreadInitializedRef.current = true;
      lastObservedActiveThreadIdRef.current = activeThreadId;
      return;
    }
    if (lastObservedActiveThreadIdRef.current === activeThreadId) {
      return;
    }
    if (activeThreadId === null) {
      // A null active thread is terminal — nothing to reposition onto.
      // Consume the observation so we do not re-enter for the same value.
      lastObservedActiveThreadIdRef.current = activeThreadId;
      return;
    }
    // Skip when the active thread change is the overlay's OWN cross-lane jump
    // echoing back: a wheel/click jump called `setActiveThread(target)`, which
    // flips this prop to `target`. That jump is already positioning the pane
    // on the user's picked message, so repositioning here would clobber it
    // with the lane's tail. Consume the observation so a later
    // `largeSortedMessages` change cannot re-enter and override the user's
    // pick. This is detected by the in-flight counter being up AND the jump's
    // recorded destination matching the new prop value.
    if (
      crossLaneJumpInFlightCountRef.current > 0 &&
      crossLaneJumpTargetThreadRef.current === activeThreadId
    ) {
      lastObservedActiveThreadIdRef.current = activeThreadId;
      return;
    }
    // A navigator-driven change to a DIFFERENT thread than any in-flight
    // overlay jump is the newest user intent — the in-flight jump (if any) is
    // now stale, so cancel it and proceed. Cancelling drives the jump's
    // `onSettled`, releasing its counter, so the guard does not stay armed
    // against this deliberate reposition.
    if (crossLaneJumpInFlightCountRef.current > 0) {
      pendingScrollCancelRef.current?.();
      pendingScrollCancelRef.current = null;
    }
    // The new lane's latest large turn = last entry in the global large
    // list whose `threadId` matches. If the lane has no large messages
    // yet (e.g. messages still loading, or the lane only carries tool
    // calls), leave the playhead alone WITHOUT consuming the observation —
    // the next render that brings the large message in re-fires this effect
    // via the `largeSortedMessages` dep and retries. Consuming here would
    // latch the observation and silently drop the retry.
    let targetUuid: string | null = null;
    for (let i = largeSortedMessages.length - 1; i >= 0; i -= 1) {
      if (largeSortedMessages[i].threadId === activeThreadId) {
        targetUuid = largeSortedMessages[i].uuid;
        break;
      }
    }
    if (targetUuid === null) {
      return;
    }
    const targetIndex = sortedMessages.findIndex((m) => m.uuid === targetUuid);
    if (targetIndex < 0) {
      return;
    }
    // A reposition is actually committing now — consume the observation so
    // this exact change is not re-processed, while an earlier bail (lane not
    // loaded) leaves it unconsumed for retry.
    lastObservedActiveThreadIdRef.current = activeThreadId;
    // This reposition is NOT a cross-lane jump, so any target recorded by a
    // previous overlay-driven jump is now stale. Clear it before raising the
    // counter below: otherwise the pane-scroll guard counter this effect holds
    // for the next {@link PANE_SCROLL_PROGRAMMATIC_GUARD_MS} would combine with
    // the stale target and make the echo-skip branch above misfire on a genuine
    // navigator pick of that same thread arriving inside the window — silently
    // swallowing the exact deliberate selection this fix must always honour.
    crossLaneJumpTargetThreadRef.current = null;
    // Suppress the pane-scroll observer for the same window the regular
    // cross-lane jump uses. The pane is about to re-render the new thread's
    // articles; without the guard, the IO's first-observation batch would
    // commit the topmost-visible article and overwrite the "latest large
    // turn" target we just committed. The counter is released by a timer
    // sized to {@link PANE_SCROLL_PROGRAMMATIC_GUARD_MS}, which comfortably
    // covers the IO debounce + the first paint of the freshly-rendered
    // pane.
    crossLaneJumpInFlightCountRef.current += 1;
    markProgrammaticScroll();
    setActiveMessageIndexFromPaneScroll(targetIndex);
    // `setActiveMessageIndexFromPaneScroll` only bumps `userActedTick` when
    // the index actually moves. The external thread switch is itself a
    // user action though — and the horizontal scroll catch-up effect is
    // gated on `userActedTick !== 0` — so bump unconditionally to make
    // sure the new lane's playhead is brought into view even when the
    // index happens to coincide with the previous active message (the
    // global tail typically lives in the latest lane, so this is the
    // common case on a freshly opened session).
    bumpUserActedTick();
    let released = false;
    const releaseOnce = () => {
      if (released) {
        return;
      }
      released = true;
      decrementCrossLaneInFlight();
    };
    if (typeof window === 'undefined' || typeof window.setTimeout !== 'function') {
      releaseOnce();
      return;
    }
    const handle = window.setTimeout(releaseOnce, PANE_SCROLL_PROGRAMMATIC_GUARD_MS);
    return () => {
      window.clearTimeout(handle);
      releaseOnce();
    };
  }, [
    activeThreadId,
    largeSortedMessages,
    sortedMessages,
    setActiveMessageIndexFromPaneScroll,
    bumpUserActedTick,
    markProgrammaticScroll,
    decrementCrossLaneInFlight,
  ]);

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
    // `onSettled` callback passed to scheduleScrollAfterRender, which fires
    // exactly once whichever way the schedule terminates — the scroll landed,
    // the DOM-ready poll timed out, or the cancel handle ran (superseding
    // jump / unmount). That guarantees this increment always has a matching
    // decrement, so the counter can never latch above zero.
    //
    // The time-based guard is stamped in the `onScroll` callback (fired only
    // when the element actually rendered and the scroll is about to land) so
    // its window covers the post-scroll IO ripple.
    //
    // CRITICAL: the time-based guard MUST be stamped in the onScroll callback
    // — NOT at jump-trigger time — because scheduleScrollAfterRender can poll
    // for many frames waiting for the new thread's re-render. If we stamped
    // the guard at trigger time the window could expire before the scroll
    // lands, leaving the post-scroll IO ripples completely unguarded. That
    // was the residual tail-jump race that survived the v12 fix.
    crossLaneJumpInFlightCountRef.current += 1;
    // Record this jump's destination lane so the external-thread effect can
    // recognise the resulting `activeThreadId` prop change as its own jump
    // echoing back (and skip it) rather than treating it as a fresh external
    // navigator pick.
    crossLaneJumpTargetThreadRef.current = current.threadId;
    // Record the navigation intent ATOMICALLY with the active-thread switch:
    // TranscriptPane's thread-change layout effect reads it synchronously in
    // the resulting commit and lands the pane on this exact message instead of
    // the newly focused lane's tail (the tail-stick writers it would otherwise
    // fire do not own this switch). The intent also keeps the pane's scroll
    // listener from re-arming stick through the landing; it is cleared a couple
    // of frames after the scroll settles (see `scheduleJumpIntentClear`).
    const jumpUuid = current.uuid;
    const jumpThreadId = current.threadId;
    setActiveThreadWithJumpTarget(jumpThreadId, jumpUuid);
    const rawCancel = scheduleScrollAfterRender(
      container,
      jumpUuid,
      () => {
        // onScroll: element is in the DOM, scrollIntoView is about to fire.
        // Stamp the time-based guard NOW so its 200ms window starts ticking
        // from the moment the IO ripples will arrive; the state-based counter
        // is released by onSettled (below), so a tail-message IO batch
        // arriving in the very next tick is suppressed by the time-based
        // guard alone.
        markProgrammaticScroll();
      },
      // onSettled: release the in-flight counter (clamped at zero, so passing
      // it after a possible cancel-handle double-fire is safe), then clear the
      // jump intent a couple of frames later so it outlives the landing scroll
      // events (which would otherwise re-arm stick).
      () => {
        decrementCrossLaneInFlight();
        scheduleJumpIntentClear(jumpUuid);
      },
      // onTimeout: the target never rendered an article (a renders-nothing
      // carrier). Park the pane on the nearest rendering lane neighbor instead
      // of leaving it at the tail. Stamp the programmatic-scroll guard first so
      // the fallback scroll does not feed the pane → playhead follower.
      () => {
        markProgrammaticScroll();
        const neighbor = nearestRenderedNeighborUuid(
          container,
          sortedMessagesRef.current,
          jumpUuid,
          jumpThreadId,
        );
        if (neighbor !== null) {
          scrollMessageIntoView(container, neighbor);
        } else if (container) {
          // No lane message rendered at all: fall back to the lane top.
          container.scrollTop = 0;
        }
      },
    );
    // The cancel handle drives onSettled itself, so it already releases the
    // counter when a superseding jump or unmount aborts the wait.
    pendingScrollCancelRef.current = rawCancel;
    // `scrubTick` is the re-trigger AND the gate: a fresh scrub bumps the
    // tick, re-fires this effect, and re-emits the (possibly identical)
    // navigation intent. A re-click at the same x bumps the tick even when
    // the active index does not move, so a stale scroll position is still
    // corrected — but a tick-less re-render never sneaks in an auto-switch
    // that the user did not ask for.
  }, [
    scrubTick,
    conversationBodyRef,
    setActiveThreadWithJumpTarget,
    markProgrammaticScroll,
    decrementCrossLaneInFlight,
    scheduleJumpIntentClear,
  ]);

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
    // The set of articles currently intersecting the viewport, keyed by uuid
    // to their viewport-relative top and bottom edges. We commit the article
    // that owns the reading-region start line per debounce tick — that is the
    // message the user is most likely reading (see `flush`). Both edges are
    // retained because the selection skips articles whose body has already
    // scrolled entirely above the reading-region line (bottom edge at/above
    // it), which needs the bottom, not just the top.
    const intersecting = new Map<string, { top: number; bottom: number }>();
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
      // Select the article that OWNS the reading-region start line, not the
      // raw smallest-top. `scrollIntoView({ block: 'start' })` parks its
      // target exactly at that line (message articles carry
      // `scroll-margin-top: var(--delta-top-region-reserve)`), which leaves
      // the PREVIOUS article still intersecting by a sliver in the reserve
      // band above the line — with a slightly smaller (more negative) top.
      // Committing raw smallest-top there would systematically pick that
      // previous article and yank the playhead one mark backwards. Instead we
      // skip any article whose bottom edge has already crossed above the line
      // (its body no longer occupies the reading region) and pick the topmost
      // of the rest. A flush that escapes the guards after a programmatic
      // scroll then resolves to the very message the scroll established — an
      // idempotent no-op — so WHEN it fires stops mattering. The line is read
      // from the same custom property the CSS uses; it degrades to 0 (plain
      // topmost-visible) when unset. Ties fall back to smallest global index
      // so the choice is deterministic.
      const reserveLine =
        container.getBoundingClientRect().top + readTopRegionReserve(container);
      let bestUuid: string | null = null;
      let bestTop = Number.POSITIVE_INFINITY;
      let bestIndex = Number.POSITIVE_INFINITY;
      for (const [uuid, { top, bottom }] of intersecting) {
        const idx = indexByUuid.get(uuid);
        if (idx === undefined) {
          continue;
        }
        // Body has fully scrolled above the reading-region line: not what the
        // user is reading, skip it. Guarded on a finite bottom so tests that
        // emit a top-only rect (bottom left `undefined` on the literal, same
        // as an unparseable NaN under `Number.isFinite`) keep plain
        // topmost-visible.
        if (Number.isFinite(bottom) && bottom <= reserveLine) {
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
            intersecting.set(uuid, {
              top: entry.boundingClientRect.top,
              bottom: entry.boundingClientRect.bottom,
            });
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
                        className={`flex h-full items-center truncate whitespace-nowrap rounded-sm py-0.5 pl-1 pr-2 font-mono text-code ${
                          lane.isMain ? 'text-fg' : 'text-fg-muted'
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
                        // `h-full` lets the axis cell expand to the grid
                        // row's stretched height (see `align-items: stretch`
                        // on the lane `<ul>`) so the axis-side highlight
                        // band lines up with the label-side band. The
                        // explicit `LANE_HEIGHT_PX` becomes a `minHeight`
                        // floor (no-content rows still respect the lane
                        // height) rather than a fixed `height` cap that
                        // would defeat the stretch.
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
                })}
              </ul>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
