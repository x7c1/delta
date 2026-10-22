import { useEffect, type MutableRefObject, type RefObject } from 'react';
import type { ThreadId } from '@delta/model';
import type { SortedMessage } from './timelineLanes';
import { ALL_ARTICLES_SELECTOR } from './timelineScroll';

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

interface TimelinePaneScrollFollowArgs {
  /** Whether the timeline is expanded; collapsed runs no follower. */
  expanded: boolean;
  /** The conversation-pane scroll container whose articles are observed. */
  conversationBodyRef: RefObject<HTMLElement | null>;
  /** Every mark in (created_at asc, seq asc) order. */
  sortedMessages: SortedMessage[];
  /** Commit a pane-driven active index without re-firing a jump. */
  setActiveMessageIndexFromPaneScroll: (next: number) => void;
  /** The active thread; a change re-binds the observer to the new DOM. */
  activeThreadId: ThreadId | null;
  /** The in-flight cross-lane jump count; above zero suppresses the follower. */
  crossLaneJumpInFlightCountRef: MutableRefObject<number>;
  /** When the latest programmatic scroll fired, or null when none is recent. */
  lastProgrammaticScrollAtRef: MutableRefObject<number | null>;
}

// Pane scroll → playhead follow (Improvement 3). Observe each rendered
// message article in the conversation pane; whichever one sits closest to
// the viewport TOP is the "current" message and drives the playhead.
// This is the bidirectional half of the sync: timeline → pane is already
// wired by the navigation effect in `useTimelinePaneJump`; this is pane →
// timeline.
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
export function useTimelinePaneScrollFollow({
  expanded,
  conversationBodyRef,
  sortedMessages,
  setActiveMessageIndexFromPaneScroll,
  activeThreadId,
  crossLaneJumpInFlightCountRef,
  lastProgrammaticScrollAtRef,
}: TimelinePaneScrollFollowArgs): void {
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
}
