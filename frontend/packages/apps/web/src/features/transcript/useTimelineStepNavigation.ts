import { useEffect, useRef, type MutableRefObject } from 'react';
import type { SortedMessage } from './timelineLanes';
import {
  WHEEL_STEP_COOLDOWN_MS,
  WHEEL_VELOCITY_WINDOW_MS,
  normalizeWheelDeltaPx,
  stepsForCumulativePx,
} from './timelineWheel';

/**
 * Shared inputs for the wheel / keyboard step-navigation hooks. Everything
 * is passed as refs (kept in sync by `ThreadTimelineOverlay`) plus the
 * stable `setActiveMessageIndex` commit callback, so the handlers always
 * read the LATEST sorted lists without re-binding their listeners on every
 * background-refetch array-identity change — exactly the discipline the
 * inline effects followed before they were extracted into these hooks.
 */
interface TimelineStepNavigationArgs {
  /** Whether the timeline footer is expanded; collapsed attaches nothing. */
  expanded: boolean;
  /** Every mark in (created_at asc, seq asc) order. */
  sortedMessagesRef: MutableRefObject<SortedMessage[]>;
  /** The main-conversation (large) subset the step navigation walks. */
  largeSortedMessagesRef: MutableRefObject<SortedMessage[]>;
  /** The active message's index in the global sorted list, or null. */
  activeMessageIndexRef: MutableRefObject<number | null>;
  /** Clamp + commit a new active index (bumps the scrub/user-acted ticks). */
  setActiveMessageIndex: (next: number) => void;
}

interface TimelineWheelStepNavigationArgs extends TimelineStepNavigationArgs {
  /** The axis scroll container the `passive: false` wheel listener binds to. */
  axisScrollRef: MutableRefObject<HTMLDivElement | null>;
}

/**
 * Wheel scrubbing over the timeline axis: discrete steps through the
 * large-message subset, with the rolling-window velocity staircase and the
 * output-side cooldown described in `timelineWheel.ts`. Extracted move-only
 * from `ThreadTimelineOverlay` — the handler body, its refs, and the effect
 * deps are unchanged.
 */
export function useTimelineWheelStepNavigation({
  expanded,
  axisScrollRef,
  sortedMessagesRef,
  largeSortedMessagesRef,
  activeMessageIndexRef,
  setActiveMessageIndex,
}: TimelineWheelStepNavigationArgs): void {
  // Rolling-window accumulator for wheel-event |delta|. Each entry is a
  // single wheel event's normalized px contribution paired with the
  // timestamp it landed on; the wheel handler evicts entries older than
  // {@link WHEEL_VELOCITY_WINDOW_MS} before reading the sum, so a multi-
  // notch spin compounds while the user's fingers are still moving but an
  // unrelated later flick always starts fresh at the slowest staircase
  // bucket. The accumulator's role replaces v4's hard cooldown: a long
  // session traverses in a handful of vigorous turns instead of dozens.
  //
  // A separate output-side gate ({@link WHEEL_STEP_COOLDOWN_MS},
  // tracked by `lastStepCommitAtMsRef` below) caps the rate at which
  // discrete step commits land. The accumulator keeps feeding during the
  // cooldown so a sustained vigorous spin still trips the higher
  // staircase buckets when the cooldown next clears — the cooldown
  // bounds throughput, not acceleration.
  const wheelWindowRef = useRef<Array<{ atMs: number; deltaPx: number }>>([]);
  // Timestamp (ms, from the same clock as the rolling-window entries) of
  // the most recent step commit emitted by the wheel handler. `null`
  // means "no prior commit", which bypasses the cooldown gate so the
  // very first event after the component mounts — or after a long pause
  // where `now - lastCommitAt` exceeds the cooldown — commits
  // immediately. A trackpad's burst inside the cooldown window still
  // feeds the accumulator (preserving staircase semantics) but does not
  // commit until the gate clears, which keeps a gentle gesture from
  // racing through multiple messages.
  const lastStepCommitAtMsRef = useRef<number | null>(null);

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
      // Output-side cooldown gate. Trackpads emit a continuous stream of
      // small pixel-mode events for a single gesture, each sub-notch and
      // therefore individually below the per-event clamp; without this
      // gate every one of those events would commit a 1-step jump and
      // the playhead would race through several messages on a gentle
      // gesture. The accumulator above is already updated this tick, so
      // the staircase still compounds across the burst — the cooldown
      // only suppresses the commit until {@link WHEEL_STEP_COOLDOWN_MS}
      // has elapsed since the last actual commit. The first event after
      // mount (or after a pause long enough that `now - lastCommitAt`
      // exceeds the cooldown) falls through immediately.
      const lastCommitAt = lastStepCommitAtMsRef.current;
      if (
        lastCommitAt !== null &&
        now - lastCommitAt < WHEEL_STEP_COOLDOWN_MS
      ) {
        return;
      }
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
      // Record the commit time so the cooldown gate above can suppress
      // any further commits for the next {@link WHEEL_STEP_COOLDOWN_MS}.
      // The ref is only written on actual commits — clamp-at-end and
      // no-op events leave the prior commit time in place, which keeps
      // the gate's "time since last visible step" reading honest.
      lastStepCommitAtMsRef.current = now;
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [expanded, setActiveMessageIndex]);
}

/**
 * ArrowLeft / ArrowRight stepping while the timeline is expanded — the
 * deterministic keyboard counterpart to the wheel scrub. Extracted move-only
 * from `ThreadTimelineOverlay`; the rationale comment below and the handler
 * body are unchanged.
 */
export function useTimelineKeyboardStepNavigation({
  expanded,
  sortedMessagesRef,
  largeSortedMessagesRef,
  activeMessageIndexRef,
  setActiveMessageIndex,
}: TimelineStepNavigationArgs): void {
  // ArrowLeft / ArrowRight step the playhead one large message per keydown
  // while the timeline is expanded. The keyboard is the deterministic
  // counterpart to the wheel: on a trackpad the inertial event stream keeps
  // feeding steps after the fingers lift (even with the output-side
  // cooldown bounding throughput), so stopping on an intended message is a
  // matter of luck — whereas one keypress is exactly one step, and holding
  // the key traverses at the OS key-repeat rate. The step semantics are
  // identical to a single wheel step: walk the large-message subset via
  // {@link pickNeighbourLargeMessage} and commit through
  // {@link setActiveMessageIndex}, so the {@link scrubTick} bump, the
  // conversation-pane jump, and cross-lane active-thread switching all
  // behave exactly as they do for wheel scrubs. Deliberately NO
  // {@link WHEEL_STEP_COOLDOWN_MS} and no velocity-window / staircase
  // machinery: keys are a deterministic input whose cadence the OS
  // key-repeat already sets, so `event.repeat` events step like any other
  // keydown.
  //
  // The listener is window-level but active only while `expanded` (the
  // keyboard analogue of scoping the wheel listener to the expanded axis
  // container): a collapsed timeline attaches nothing, so arrow keys fall
  // through to the page untouched. There is no global keyboard-shortcut
  // registry in the web app today — existing keydown handling is all
  // element-local React `onKeyDown` — so this effect attaches its own
  // listener directly, the same way the wheel effect manages its own,
  // rather than plugging into a shared shortcut layer.
  useEffect(() => {
    if (!expanded) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      // Only plain ArrowLeft / ArrowRight are ours. A Ctrl / Meta / Alt
      // chord belongs to the browser or OS (Alt+Arrow word-jump, Cmd+Arrow
      // line-edge, ...), and a key some earlier handler already claimed
      // (`defaultPrevented`) stays claimed.
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') {
        return;
      }
      if (event.ctrlKey || event.metaKey || event.altKey) {
        return;
      }
      if (event.defaultPrevented) {
        return;
      }
      // Never hijack text entry or the terminal — see
      // {@link isEditableEventTarget} for what counts as editable.
      if (isEditableEventTarget(event.target)) {
        return;
      }
      // Past the guards the keypress belongs to the timeline: suppress the
      // page-level default (horizontal scrolling) even when the step below
      // clamps into a no-op at either end.
      event.preventDefault();
      const total = sortedMessagesRef.current.length;
      const large = largeSortedMessagesRef.current;
      if (total === 0 || large.length === 0) {
        return;
      }
      // ArrowRight → newer (timeline right), ArrowLeft → older — mirroring
      // the visual axis (left = past, right = latest) and the wheel's
      // positive-delta = newer convention. Clamped at the ends — no wrap.
      const direction: 1 | -1 = event.key === 'ArrowRight' ? 1 : -1;
      const currentIndex = activeMessageIndexRef.current ?? total - 1;
      const currentMessage = sortedMessagesRef.current[currentIndex];
      const next = pickNeighbourLargeMessage(large, currentMessage, direction);
      if (next === null) {
        return;
      }
      const nextGlobalIndex = sortedMessagesRef.current.findIndex(
        (m) => m.uuid === next.uuid,
      );
      if (nextGlobalIndex < 0 || nextGlobalIndex === currentIndex) {
        return;
      }
      setActiveMessageIndex(nextGlobalIndex);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [expanded, setActiveMessageIndex]);
}

/**
 * Whether a keydown targeting this element is text entry the timeline's
 * keyboard navigation must never hijack: form controls (`input`,
 * `textarea`, `select`) and anything inside a contentEditable region.
 * `isContentEditable` is inherited — an element inside an editable
 * ancestor reports `true` — so the single property read covers "anything
 * inside" without walking ancestors. Non-element targets (`window`,
 * `document`) are trivially not editable.
 *
 * The `textarea` case covers both the composer AND xterm's hidden helper
 * textarea (`.xterm-helper-textarea` in TerminalPane), which is how all
 * keyboard input reaches the terminal — so arrow keys typed into the
 * terminal are never swallowed by the timeline.
 */
function isEditableEventTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  const tag = target.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') {
    return true;
  }
  return target instanceof HTMLElement && target.isContentEditable;
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
