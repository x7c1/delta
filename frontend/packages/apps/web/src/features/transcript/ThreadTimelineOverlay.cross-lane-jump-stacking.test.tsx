/**
 * Cross-lane jumps: the guard hand-off between jumps, and stacked jumps.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import {
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query';
import { act, screen, waitFor, within } from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { ThreadTimelineOverlay } from './ThreadTimelineOverlay';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
import {
  PANE_SCROLL_DEBOUNCE_MS,
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
} from './useTimelinePaneScrollFollow';
import { WHEEL_STEP_COOLDOWN_MS } from './timelineWheel';
import {
  makeThread,
  makeUserText,
  playheadLeftPx,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';
import { TIMERS_ONLY } from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay cross-lane jump guard hand-off and stacked jumps', () => {
  // Regression suite for the residual tail-jump race that survived v12.
  //
  // Symptom (user dogfooding): a fast wheel chain that crossed lanes —
  // especially child thread → parent thread — still occasionally snapped
  // the playhead to the tail of the new lane.
  //
  // Two compounding root causes:
  //
  // (1) v12 stamped markProgrammaticScroll at jump-trigger time, BEFORE
  //     scheduleScrollAfterRender polled the DOM for the target element.
  //     When the cross-lane re-render took longer than
  //     PANE_SCROLL_PROGRAMMATIC_GUARD_MS (200 ms), the time-based guard
  //     window had already expired by the time the onScroll callback fired
  //     and released the state-based flag. The IO ripples from the actual
  //     scrollIntoView then arrived into a fully-unguarded flush, and the
  //     tail of the new lane was committed as the active index.
  //
  //     v12 closed the "during the wait" gap with the state-based flag, but
  //     the "right after the scroll" gap was still open — the hand-off from
  //     state-based guard to time-based guard was broken because the
  //     time-based guard's window had already expired.
  //
  //     v13 fix: stamp markProgrammaticScroll inside the onScroll callback,
  //     adjacent to where the counter is released — so the 200 ms window
  //     starts ticking from the moment the IO ripples actually begin.
  //
  // (2) v12 used a boolean in-flight flag, which a stacked burst of cross-
  //     lane jumps could mishandle: the first jump's onScroll would clear
  //     the flag while a later jump was still polling, opening the same race
  //     window the guard exists to close.
  //
  //     v13 fix: replace the boolean with a counter — the guard only
  //     releases when EVERY in-flight jump has settled.

  type FakeIO = {
    callback: IntersectionObserverCallback;
    options?: IntersectionObserverInit;
    observed: Set<Element>;
    emit: (entries: Partial<IntersectionObserverEntry>[]) => void;
  };

  function installFakeIO(): { instances: FakeIO[]; restore: () => void } {
    const instances: FakeIO[] = [];
    const original = (
      globalThis as { IntersectionObserver?: typeof IntersectionObserver }
    ).IntersectionObserver;
    class FakeIntersectionObserver {
      callback: IntersectionObserverCallback;
      options?: IntersectionObserverInit;
      observed = new Set<Element>();
      emit!: (entries: Partial<IntersectionObserverEntry>[]) => void;
      constructor(
        cb: IntersectionObserverCallback,
        opts?: IntersectionObserverInit,
      ) {
        this.callback = cb;
        this.options = opts;
        this.emit = (entries) => {
          this.callback(
            entries as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        };
        instances.push(this as unknown as FakeIO);
      }
      observe(el: Element) { this.observed.add(el); }
      unobserve(el: Element) { this.observed.delete(el); }
      disconnect() { this.observed.clear(); }
      takeRecords(): IntersectionObserverEntry[] { return []; }
    }
    (globalThis as { IntersectionObserver?: unknown }).IntersectionObserver =
      FakeIntersectionObserver;
    return {
      instances,
      restore: () => {
        (
          globalThis as {
            IntersectionObserver?: typeof IntersectionObserver | undefined;
          }
        ).IntersectionObserver = original;
      },
    };
  }

  /**
   * Resolves the LIVE FakeIO that observes every expected article. The
   * timeline effect re-runs as async query results settle and replaces its
   * IntersectionObserver each time, leaving earlier instances disconnected
   * (observed set cleared). Capturing `fake.instances[length - 1]`
   * synchronously races that replacement: between the capture and the
   * subsequent `io.emit(...)`, a later effect-run can wedge a new live
   * observer ahead of the one we grabbed, and the emit then reaches a dead
   * observer whose callback no longer fires.
   *
   * Polls until the most recent FakeIO observes `expectedObserved`
   * elements — the live observer always re-`observe()`s every article on
   * construction.
   */
  async function getLiveIO(
    fake: { instances: FakeIO[] },
    expectedObserved: number,
  ): Promise<FakeIO> {
    return waitFor(() => {
      const candidate = fake.instances.at(-1);
      expect(candidate?.observed.size).toBe(expectedObserved);
      return candidate as FakeIO;
    });
  }

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('suppresses IO ripples that arrive after a slow cross-lane scroll lands (time-based guard starts at scroll-fire, not jump-trigger)', async () => {
    // This is the regression test that pins root cause (1). Sequence:
    //
    //   1. Wheel-up → cross-lane jump child (lane 2) → parent (lane 1).
    //   2. setActiveThread(1) fires. DOM-ready poll begins; the parent's
    //      re-render is slow, taking 500ms (well past the 200ms time-based
    //      guard). The in-flight counter holds the IO at bay during the wait.
    //   3. The poll finds msg-a in the DOM at +500ms; the onScroll callback
    //      stamps markProgrammaticScroll AT THAT MOMENT and releases the
    //      counter. scrollIntoView fires.
    //   4. The scroll's IO ripples arrive a few ms later. They must be
    //      blocked by the time-based guard, whose window now starts at +500
    //      (not at the jump trigger), so a flush at +550 is still inside
    //      the 200ms window.
    //
    // If v12's stamping (markProgrammaticScroll at jump-trigger) were still
    // in place, the time-based guard would have armed at t=0, expired at
    // t=200, and the flush at t=550 would commit the tail message.
    const fake = installFakeIO();
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* no-op */
    }) as typeof window.cancelAnimationFrame;
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Parent thread (lane 1) holds three messages: msg-a (target), an
      // intermediate, and msg-tail (the tail message that the IO would
      // spuriously commit if the guard is broken). Child thread (lane 2)
      // holds msg-c, which is the initial active message.
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:03:00Z',
        }),
      ];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-mid', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'msg-tail', '2026-01-01T00:02:00Z'),
          ],
        ],
        [2, [makeUserText(2, 0, 'msg-c', '2026-01-01T00:04:00Z')]],
      ]);
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial playhead is on msg-c (the latest, x=240).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

      // Wheel-up: one sub-notch step back → cross-lane jump child → parent.
      // The global sorted order is by (created_at, seq), so backwards from
      // msg-c (the initial active message) is msg-tail. The jump target is
      // therefore msg-tail (x derived from its timestamp fraction along the
      // shared global axis). The bug we want to pin is NOT about the jump
      // target being wrong; it's about a SUBSEQUENT IO emit for some OTHER
      // article in the parent lane (e.g. msg-a) snapping the playhead. So
      // after the jump lands on msg-tail, we simulate an IO emit for msg-a
      // (which would, in the broken case, commit msg-a as the new active
      // index because the guard is expired). The expectation is: playhead
      // stays on msg-tail.
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // The active thread flips to lane 1.
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // Re-render the pane with the parent lane's articles. The target
      // (msg-tail at x=160) will land at the playhead; we'll then simulate
      // a spurious IO emit for msg-a as the "topmost-visible" entry that
      // the broken hand-off would commit.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-mid">msg-mid</article>
                <article data-message-uuid="msg-tail">msg-tail</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Simulate a slow parent re-render: advance the clock 500ms BEFORE
      // draining the rAF queue. The in-flight counter holds during this
      // window — assert it by emitting an IO entry for msg-a now and
      // confirming the flush bails.
      nowMs += 500;
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      const msgAArticle = articles.find((a) => a.textContent === 'msg-a')!;
      vi.useFakeTimers(TIMERS_ONLY);
      try {
        act(() => {
          io.emit([
            {
              target: msgAArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Counter is still held — playhead stays on msg-tail (x=120, derived
      // from its timestamp fraction along the 240px axis: 120s / 240s range).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);

      // Now drain the rAF callbacks so the DOM-ready poll fires onScroll.
      // At this moment markProgrammaticScroll stamps the time-based guard
      // with nowMs=10_500 (the slow-re-render timestamp), the counter
      // releases, and scrollIntoView lands.
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }

      // Advance the clock a small amount (less than the time-based guard
      // window) and emit another IO entry for msg-a — this simulates the
      // scroll's own IO ripple. The flush must bail because the time-based
      // guard window started at nowMs=10_500 and has not yet expired.
      nowMs += 50;
      vi.useFakeTimers(TIMERS_ONLY);
      try {
        act(() => {
          io.emit([
            {
              target: msgAArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Playhead is still on msg-tail (x=120) — the time-based guard caught
      // the post-scroll IO ripple. In the broken v12 hand-off the playhead
      // would have snapped to msg-a (x=0).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('keeps the in-flight counter balanced across a cross-lane chain so a later jump still guards correctly (settle-once, no double-decrement)', async () => {
    // The counter is decremented by ONE mechanism: scheduleScrollAfterRender's
    // `onSettled` callback (here, `decrementCrossLaneInFlight`), which fires at
    // most once per jump no matter which of its termination paths runs first —
    // the scroll landed, the DOM-ready poll timed out, or the returned cancel
    // handle was invoked. The cancel handle drives that SAME internal
    // settle-once guard, so every wheel-step beyond the first invokes the
    // previous jump's cancel handle EVEN IF that jump's onSettled has already
    // fired, and the decrement still only happens once per jump.
    //
    // The observable consequence of a regression here would be: the
    // decrementCrossLaneInFlight clamp prevents the counter from wrapping
    // below zero, but the missed accounting hides a deeper bug — a future
    // jump's increment may end up "absorbing" an earlier missed decrement,
    // leaving the counter at zero when it should be at one and releasing
    // the guard prematurely.
    //
    // We exercise the end-to-end shape: a chain of three cross-lane jumps
    // (lane 3 → lane 2 → lane 1 → lane 2). Each subsequent jump invokes the
    // prior jump's cancel handle after its onSettled has already fired. If
    // the settle-once guard were missing, the third jump's counter would not
    // properly block an IO emit fired before its rAF poll drains.
    const fake = installFakeIO();
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* no-op */
    }) as typeof window.cancelAnimationFrame;
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Three single-message lanes so each wheel-step is a cross-lane jump:
      //   lane 1: msg-a (00:00)
      //   lane 2: msg-b (00:01)
      //   lane 3: msg-c (00:02)
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
        makeThread(3, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:02:00Z',
        }),
      ];
      const messages = new Map([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z')]],
        [3, [makeUserText(3, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 3,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');

      const axisColumn = screen.getAllByTestId(
        'thread-timeline-axis-column',
      )[0];

      // Jump 1: wheel-up cross-lane msg-c → msg-b (lane 3 → lane 2). Counter
      // 0 → 1.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // Advance past the wheel-handler cooldown so subsequent wheel
      // events in this chain are not throttled — the cooldown is
      // unrelated to the counter-balance contract this test exercises.
      nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      // Re-render with lane 2's article so the DOM-ready poll finds msg-b.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={2}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Drain rAFs → onScroll fires for jump 1, counter 1 → 0.
      let drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }

      // Jump 2: wheel-up cross-lane msg-b → msg-a (lane 2 → lane 1). The
      // navigation effect invokes jump 1's cancel handle FIRST. Jump 1 has
      // already settled (onScroll fired, onSettled decremented the counter),
      // so a missing settle-once guard would attempt a second decrement on
      // jump 1 via that cancel handle — the clamp prevents wrap, but the
      // accounting is now off-by-one.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // Advance past the wheel-handler cooldown again before the third
      // jump fires below.
      nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }

      // Jump 3: wheel-down cross-lane msg-a → msg-b (lane 1 → lane 2). The
      // navigation effect invokes jump 2's cancel handle (jump 2 already
      // settled). With the settle-once guard, the counter should now be 1
      // (jump 3 incremented, jump 2's cancel no-ops). Verify by emitting an
      // IO entry BEFORE draining rAFs — the flush must bail.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: 50, bubbles: true, cancelable: true }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={2}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Advance past the time-based guard window so ONLY the counter could
      // block the IO. With the settle-once guard preserving accounting, the
      // counter is at 1; flush bails. Without it the counter is at 0 and the flush
      // would commit msg-b's IO entry (a no-op visually since the playhead
      // is already at msg-b, so we choose msg-a as the IO target — which
      // would snap the playhead BACK to msg-a if the guard were broken).
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
      // The pane only contains msg-b right now, so IO can only emit on
      // msg-b. We use msg-b as the IO target. Because msg-b is also the
      // playhead's current position, an honoured flush would be a no-op
      // visually — that doesn't distinguish "bailed" from "honoured-on-
      // same-index". So instead, observe whether the flush would mark a
      // pane-scroll-driven change by checking that the playhead does not
      // shift. We strengthen the assertion by emitting an entry for msg-a
      // via a stand-in article appended manually below the visible one;
      // this exercises the "topmost visible is the older message" path.
      const standin = document.createElement('article');
      standin.setAttribute('data-message-uuid', 'msg-a');
      standin.textContent = 'msg-a';
      screen.getByTestId('conversation-body').appendChild(standin);
      // Observe the new article: the MutationObserver in the IO effect
      // catches DOM mutations and starts observing. Allow a microtask for
      // the observation to register, then resolve the live IO once both
      // msg-b and the stand-in are observed.
      await Promise.resolve();
      const io = await getLiveIO(fake, 2);
      vi.useFakeTimers(TIMERS_ONLY);
      try {
        act(() => {
          io.emit([
            {
              target: standin,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // msg-b's x in the 3-message axis: 00:00 / 00:01 / 00:02 → range
      // 120s → msg-b at 60/120 * 240 = 120px. Playhead stays at msg-b
      // (x=120). If the counter had double-decremented and reached 0, the
      // emit would have snapped the playhead to msg-a (x=0).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });
});
