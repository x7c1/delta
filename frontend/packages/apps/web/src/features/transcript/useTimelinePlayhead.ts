import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import type { ThreadId } from '@delta/model';
import type { SortedMessage } from './timelineLanes';

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

interface TimelinePlayheadArgs {
  /** Every mark in (created_at asc, seq asc) order. */
  sortedMessages: SortedMessage[];
  /** The main-conversation (large) subset. */
  largeSortedMessages: SortedMessage[];
  /** The active thread the auto-anchor lands on. */
  activeThreadId: ThreadId | null;
  /** Layout-phase mirror of `sortedMessages` for native-listener commits. */
  sortedMessagesRef: RefObject<SortedMessage[]>;
}

/**
 * The playhead's state: the active message (held as its UUID), the ticks that
 * record user navigation, the two commit paths, and the auto-anchor that
 * positions the playhead until the user acts.
 */
export function useTimelinePlayhead({
  sortedMessages,
  largeSortedMessages,
  activeThreadId,
  sortedMessagesRef,
}: TimelinePlayheadArgs) {
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
  // The latest pick, written synchronously by {@link setActiveMessageUuid}
  // before React renders it. The setters compare against it instead of
  // reading `prev` inside a state updater: React may defer an updater to the
  // next render, so a "did it change?" flag set inside one is not reliably
  // visible to the caller.
  const activeMessageUuidRef = useRef(activeMessageUuid);
  const setActiveMessageUuid = useCallback((uuid: string | null) => {
    activeMessageUuidRef.current = uuid;
    setActiveMessageUuidState(uuid);
  }, []);

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
  // thread-switch + scroll effect (in `useTimelinePaneJump`) uses it as a
  // re-trigger so a re-click at the playhead's current position (and thus the
  // current active index) still re-fires the jump — without it React would bail
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
      setActiveMessageUuid(list[clamped].uuid);
    },
    [bumpUserActedTick, setActiveMessageUuid],
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
      // Bump the "user has acted" gate only when we actually move the
      // playhead — repeat IO entries for the same topmost message should not
      // keep flipping the auto-anchor gate on every burst.
      if (activeMessageUuidRef.current === uuid) {
        return;
      }
      setActiveMessageUuid(uuid);
      bumpUserActedTick();
    },
    [sortedMessages, bumpUserActedTick, setActiveMessageUuid],
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
    setActiveMessageUuid(
      resolveAutoAnchorUuid(
        sortedMessages,
        largeSortedMessages,
        activeThreadId,
        activeMessageUuidRef.current,
      ),
    );
  }, [
    sortedMessages,
    largeSortedMessages,
    activeThreadId,
    setActiveMessageUuid,
  ]);

  return {
    activeMessageIndex,
    scrubTick,
    userActedTick,
    bumpUserActedTick,
    setActiveMessageIndex,
    setActiveMessageIndexFromPaneScroll,
    activeMessage,
    activeMessageRef,
  };
}
