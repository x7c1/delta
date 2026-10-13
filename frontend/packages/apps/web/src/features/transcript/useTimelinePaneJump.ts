import {
  useCallback,
  useEffect,
  useRef,
  type MutableRefObject,
  type RefObject,
} from 'react';
import type { ThreadId } from '@delta/model';
import { useNavStore } from '../../store/navStore';
import type { SortedMessage } from './timelineLanes';
import {
  highlightMessageJump,
  nearestRenderedNeighborUuid,
  scheduleScrollAfterRender,
  scrollMessageIntoView,
} from './timelineScroll';
import {
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
} from './useTimelinePaneScrollFollow';

interface TimelinePaneJumpArgs {
  /** The active thread (the navigator's pick, or the overlay's own jump). */
  activeThreadId: ThreadId | null;
  /** The conversation-pane scroll container a jump scrolls. */
  conversationBodyRef: RefObject<HTMLElement | null>;
  /** Every mark in (created_at asc, seq asc) order. */
  sortedMessages: SortedMessage[];
  /** The main-conversation (large) subset. */
  largeSortedMessages: SortedMessage[];
  /** Layout-phase mirror of `sortedMessages`. */
  sortedMessagesRef: MutableRefObject<SortedMessage[]>;
  /** Bumped by every user-driven jump; re-fires the jump effect. */
  scrubTick: number;
  /** The latest active message, read without re-firing on identity changes. */
  activeMessageRef: MutableRefObject<SortedMessage | null>;
  /** Commit an active index without re-firing a jump. */
  setActiveMessageIndexFromPaneScroll: (next: number) => void;
  /** Mark that the user has acted on the timeline. */
  bumpUserActedTick: () => void;
}

/**
 * Timeline → pane navigation: the jump that scrolls the conversation pane to
 * the playhead's message (switching lanes when it lives on another thread),
 * the reposition onto a lane the navigator picked, and the guards that keep
 * those programmatic scrolls from feeding back into the playhead. Returns the
 * two guards the pane → playhead follower reads.
 */
export function useTimelinePaneJump({
  activeThreadId,
  conversationBodyRef,
  sortedMessages,
  largeSortedMessages,
  sortedMessagesRef,
  scrubTick,
  activeMessageRef,
  setActiveMessageIndexFromPaneScroll,
  bumpUserActedTick,
}: TimelinePaneJumpArgs) {
  const setActiveThreadWithJumpTarget = useNavStore(
    (state) => state.setActiveThreadWithJumpTarget,
  );
  const clearActiveThreadJumpTarget = useNavStore(
    (state) => state.clearActiveThreadJumpTarget,
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
  // does, however, trigger the horizontal scroll catch-up effect (in
  // `useTimelineScrollCatchUp`) — so the playhead's new x is brought into view
  // automatically. The pane-scroll observer is fenced off for a short window
  // via {@link crossLaneJumpInFlightCountRef} so the IO's first-observation batch
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

  return { crossLaneJumpInFlightCountRef, lastProgrammaticScrollAtRef };
}
