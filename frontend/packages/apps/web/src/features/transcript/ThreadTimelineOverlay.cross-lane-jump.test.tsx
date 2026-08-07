/**
 * Cross-lane jump IntersectionObserver guards and the
 * article-anchored uuid selector.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import { createRef } from 'react';
import {
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query';
import {
  act,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
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
import {
  LANE_LEFT_PAD_PX,
  PANE_SCROLL_DEBOUNCE_MS,
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
  ThreadTimelineOverlay,
} from './ThreadTimelineOverlay';
import {
  ALL_ARTICLES_SELECTOR,
  articleMessageSelector,
  scrollMessageIntoView,
} from './timelineScroll';
import { WHEEL_STEP_COOLDOWN_MS } from './timelineWheel';
import {
  makeMessage,
  makeThread,
  makeUserText,
  playheadLeftPx,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay cross-lane jump IO guard (v12)', () => {
  // Regression suite for the tail-jump race: a cross-lane wheel-scrub near
  // the left edge occasionally snapped the playhead to the right-edge (tail)
  // message. Root cause: markProgrammaticScroll was called at jump-trigger
  // time, but the actual scrollIntoView only fires after DOM-ready polling
  // (scheduleScrollAfterRender). The 200 ms time-based guard expired during
  // slow re-renders; the IO's first-observation batch on the new thread's
  // articles (which always includes the tail if the pane is freshly rendered
  // at the bottom) slipped through and committed the tail index.
  //
  // The v12 fix introduced a state-based in-flight guard that holds from the
  // moment setActiveThread fires until scrollIntoView fires (or the jump is
  // cancelled). v13 changed the guard from a boolean to a counter (so a
  // burst of stacked jumps is tracked correctly) and moved
  // markProgrammaticScroll into the onScroll callback (so the 200ms time-
  // based guard window starts ticking from the moment the scroll actually
  // lands, not from the moment the jump was triggered). The IO flush bails
  // immediately while the counter is non-zero, regardless of elapsed time.

  /**
   * Fake IO helper shared with the v11 tests above. Defined locally here so
   * the suite is self-contained.
   */
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

  it('suppresses IO updates fired after the time-based guard expires but before the cross-lane scroll completes', async () => {
    // This test pins the v12 fix: when a cross-lane jump triggers a slow
    // re-render (> PANE_SCROLL_PROGRAMMATIC_GUARD_MS), the IO's first-
    // observation batch on the new thread's articles must still be blocked
    // by the state-based in-flight counter, even though the time-based
    // guard has already expired.
    //
    // Sequence simulated:
    //   1. Click → cross-lane jump to lane 2 (playhead = msg-a, index 0).
    //   2. setActiveThread(2) fires; re-render takes longer than the guard.
    //   3. IO re-binds on the new thread. IO fires: tail (msg-b) is visible.
    //   4. Debounce fires — with only the time-based guard this would commit
    //      msg-b (tail), snapping the playhead to the right edge.
    //   5. With the state-based guard (flag still true), flush bails.
    //   6. The DOM-ready poll finds msg-a → scroll fires → flag clears.
    //   7. A subsequent IO emit is now honoured normally.
    const fake = installFakeIO();
    // Capture rAF callbacks to drive the DOM-ready poll manually.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised in this test */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so we can make the guard window expire on demand.
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Lane 1 holds msg-a (old subthread, left edge); lane 2 holds msg-b
      // (new subthread, right/tail). The initial playhead lands on msg-b
      // (latest), so a wheel-up from msg-b → msg-a triggers a cross-lane
      // jump to lane 1.
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      const messages = new Map([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:02:00Z')]],
      ]);
      // Start with lane 2 active; msg-b is in the pane.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-b' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial playhead anchors to lane 2’s latest large turn (msg-b, x=240).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

      // Wheel-up: one sub-notch step back → cross-lane jump to msg-a on lane 1.
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // The active thread flips to lane 1 immediately.
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // The IO re-binds now that activeThreadId changed. Re-render the pane
      // with both articles so msg-b (the tail) is visible first.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                {/* msg-a is the jump target but msg-b (tail) is also visible */}
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-b">msg-b</article>
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
      // Advance past the time-based guard window so only the state-based
      // flag remains to block the flush.
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;

      // Simulate the IO firing for msg-b (tail) — exactly what happens when
      // the new thread's pane is freshly rendered with the tail message at
      // the bottom of the viewport.
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      const msgBArticle = articles.find((a) => a.textContent === 'msg-b')!;
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgBArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        // Advance debounce — the flush fires but the state-based guard
        // blocks it. The playhead must stay at msg-a (x=0), not jump to
        // msg-b (x=240).
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // The playhead is still at msg-a's x (0) — the state-based guard held.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);

      // Now drain the rAF callbacks so the DOM-ready scroll fires (msg-a is
      // already in the DOM), clearing the in-flight counter. As of v13 the
      // onScroll callback also stamps markProgrammaticScroll at this moment,
      // so the time-based guard window starts ticking now (not at jump
      // trigger). Advance past it before the next emit so we are testing
      // counter-release alone (the v12 contract), not the v13 time-based
      // guard hand-off.
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
      // After the counter releases AND the time-based guard expires, a
      // genuine IO emit (user manually scrolling) IS honoured normally.
      // Emit msg-b again as if the user scrolled down.
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgBArticle,
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
      // The emit is now honoured — playhead moves to msg-b (x=240).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('clears the in-flight flag immediately when a superseding jump cancels the pending scroll', async () => {
    // If a second jump fires before the first DOM-ready poll completes
    // (the user scrubs again before the re-render lands), the cancel handle
    // for the first jump must clear the flag — otherwise the flag would
    // permanently block pane → timeline sync.
    const fake = installFakeIO();
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so the two back-to-back wheel-up events sit
    // far enough apart that the wheel handler's output cooldown does
    // not suppress the second commit. The cooldown is unrelated to the
    // in-flight-flag race this test exercises, but the synchronous
    // dispatch order makes the gap effectively zero without a mock.
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Three messages across two lanes. Cross-lane jump: msg-c → msg-b
      // (lane 2 → lane 1). Then a second wheel-up supersedes it (msg-b →
      // msg-a, same lane 1).
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
        [2, [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
      // Start on lane 2 with msg-c active.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');

      // First wheel-up: cross-lane jump msg-c → msg-b (lane 2 → lane 1).
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // rAF callbacks are queued; the DOM-ready poll is in flight.
      expect(rafCallbacks.length).toBeGreaterThan(0);

      // Supersede the first jump with a second wheel-up while msg-a and
      // msg-b are now in the DOM (same-lane jump msg-b → msg-a). The
      // pending scroll cancel is invoked by the navigation effect, which
      // clears the in-flight flag.
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
                <article data-message-uuid="msg-b">msg-b</article>
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
      // Push the simulated clock past the output cooldown so the second
      // wheel commit lands.
      nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // The playhead is now on msg-a (the oldest message, x=0).
      await waitFor(() => {
        expect(
          playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
        ).toBe(`${LANE_LEFT_PAD_PX}px`);
      });

      // After the superseding jump, the in-flight flag must be cleared so
      // subsequent IO emits are honoured. Emit msg-b (index 1, x=120px) as
      // topmost-visible. The same-lane scroll from the superseding jump also
      // set the time-based guard; advance performance.now past that window
      // too so only the in-flight flag would block the flush (confirming it
      // is cleared by the cancel-with-flag-clear wrapper).
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      // msg-b is the article at index 1 in sortedMessages (x=120px on a
      // 3-message axis of width 240px: 0, 120, 240). Using msg-b (not the
      // tail) keeps the expectation non-trivial: if the flag were NOT cleared,
      // the flush would bail, and the playhead would stay at 0px.
      const msgBArticle = articles.find((a) => a.textContent === 'msg-b')!;
      vi.useFakeTimers();
      try {
        // Advance past the time-based guard window so only the state-based
        // flag (if still set) could block the flush.
        nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
        act(() => {
          io.emit([
            {
              target: msgBArticle,
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
      // The emit is honoured — the in-flight counter was decremented by the
      // cancel handle when the superseding jump fired, and the time-based
      // guard has also expired. msg-b sits at index 1 → x=120px on the
      // shared axis.
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

describe('ThreadTimelineOverlay cross-lane jump IO guard (v13)', () => {
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
      vi.useFakeTimers();
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
      vi.useFakeTimers();
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
      vi.useFakeTimers();
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

describe('ThreadTimelineOverlay article-anchored uuid selector (v16)', () => {
  // v14 pt.2 moved the timeline into the conversation pane's scroll
  // container; v15 then sticky-pinned that container to the top of the
  // scroll viewport. Both `TimelineDotMark` and `TimelineClusterMark`
  // stamp `data-message-uuid` (the dot/cluster identity matches its
  // representative message), so a bare `[data-message-uuid="X"]` query
  // rooted at the container hits the timeline span first in DOM-pre-
  // order — the span lives in the sticky topRegion that renders before
  // the article list. The result was a double regression:
  //   - timeline click → conversation pane no longer scrolled to the
  //     targeted message (scrollIntoView landed on the already-visible
  //     dot, a no-op).
  //   - conversation pane scroll → timeline playhead no longer followed
  //     (the IntersectionObserver observed the sticky dots, which always
  //     win the topmost-visible race at top: 0).
  // v16 pins every uuid query to the `<article>` tag — the only tag
  // `MessageItem` ever renders — via `articleMessageSelector` /
  // `ALL_ARTICLES_SELECTOR`. These tests reproduce the regression by
  // building a container that holds both an article and a span sharing
  // the same uuid, then assert that v16's selectors only ever pick the
  // article.

  beforeEach(() => {
    resetGlobals();
  });

  it('articleMessageSelector matches an <article> with the uuid but not a <span> with the same uuid', () => {
    // Container modelling the live layout shape: the sticky region with
    // the timeline span sits first; the message article sits below it,
    // both inside the same conversation-pane scroll container.
    const container = document.createElement('div');
    const dot = document.createElement('span');
    dot.setAttribute('data-message-uuid', 'msg-X');
    dot.setAttribute('data-testid', 'thread-timeline-dot');
    container.appendChild(dot);
    const article = document.createElement('article');
    article.setAttribute('data-message-uuid', 'msg-X');
    container.appendChild(article);
    document.body.appendChild(container);
    try {
      const sel = articleMessageSelector('msg-X');
      const matches = container.querySelectorAll(sel);
      expect(matches.length).toBe(1);
      // Article wins, not the timeline span — even though the span comes
      // first in DOM-pre-order.
      expect(matches[0]).toBe(article);
      // And the selector itself is shaped as `article[data-message-uuid="X"]`
      // so a future regression that drops the tag anchor is visible.
      expect(sel.startsWith('article[')).toBe(true);
    } finally {
      container.remove();
    }
  });

  it('ALL_ARTICLES_SELECTOR observes only article elements with a uuid, never timeline dots or clusters', () => {
    const container = document.createElement('div');
    const dot = document.createElement('span');
    dot.setAttribute('data-message-uuid', 'msg-X');
    dot.setAttribute('data-testid', 'thread-timeline-dot');
    container.appendChild(dot);
    const cluster = document.createElement('span');
    cluster.setAttribute('data-message-uuid', 'msg-X');
    cluster.setAttribute('data-testid', 'thread-timeline-cluster');
    container.appendChild(cluster);
    const articleA = document.createElement('article');
    articleA.setAttribute('data-message-uuid', 'msg-X');
    container.appendChild(articleA);
    const articleB = document.createElement('article');
    articleB.setAttribute('data-message-uuid', 'msg-Y');
    container.appendChild(articleB);
    document.body.appendChild(container);
    try {
      const matches = Array.from(
        container.querySelectorAll(ALL_ARTICLES_SELECTOR),
      );
      expect(matches).toHaveLength(2);
      expect(matches).toContain(articleA);
      expect(matches).toContain(articleB);
      expect(matches).not.toContain(dot);
      expect(matches).not.toContain(cluster);
    } finally {
      container.remove();
    }
  });

  it('scrollMessageIntoView jumps to the article, not the timeline dot, when both share the uuid in the container', () => {
    // The regression test for Issue 1: in the live app the timeline
    // sits inside the conversation pane via TranscriptPane's sticky
    // topRegion, so the conversation body contains BOTH the article
    // (the scroll target) and the dot (the timeline identity). Before
    // v16 the unscoped selector grabbed the dot, leaving the scroll a
    // no-op. After v16 the article-anchored selector picks the article.
    const container = document.createElement('div');
    const dotScrollSpy = vi.fn();
    const articleScrollSpy = vi.fn();
    const dot = document.createElement('span');
    dot.setAttribute('data-message-uuid', 'msg-X');
    dot.scrollIntoView = dotScrollSpy as unknown as Element['scrollIntoView'];
    container.appendChild(dot);
    const article = document.createElement('article');
    article.setAttribute('data-message-uuid', 'msg-X');
    article.scrollIntoView =
      articleScrollSpy as unknown as Element['scrollIntoView'];
    container.appendChild(article);
    document.body.appendChild(container);
    try {
      scrollMessageIntoView(container, 'msg-X');
      // The article is the scroll target; the dot is left untouched.
      expect(articleScrollSpy).toHaveBeenCalledTimes(1);
      expect(articleScrollSpy).toHaveBeenCalledWith({ block: 'start' });
      expect(dotScrollSpy).not.toHaveBeenCalled();
    } finally {
      container.remove();
    }
  });

  it('the pane-scroll IntersectionObserver only observes article message elements, never timeline marks, even when both share the uuid in the same container', async () => {
    // The regression test for Issue 2: before v16 the IO's
    // `querySelectorAll('[data-message-uuid]')` picked up the timeline
    // dots inside the sticky region, and those dots always reported
    // `boundingClientRect.top` near 0 (sticky-pinned), so the
    // topmost-visible race froze the playhead on a dot's uuid and the
    // playhead never followed the user's pane scroll. After v16 the
    // article-anchored selector keeps the dots out of the observation
    // set entirely.
    const fakeIO = {
      instances: [] as Array<{
        observed: Set<Element>;
      }>,
      restore: () => undefined as void,
    };
    const original = (
      globalThis as { IntersectionObserver?: typeof IntersectionObserver }
    ).IntersectionObserver;
    class FakeIO {
      observed = new Set<Element>();
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      constructor(_cb: IntersectionObserverCallback, _opts?: IntersectionObserverInit) {
        fakeIO.instances.push(this);
      }
      observe(el: Element) {
        this.observed.add(el);
      }
      unobserve(el: Element) {
        this.observed.delete(el);
      }
      disconnect() {
        this.observed.clear();
      }
      takeRecords(): IntersectionObserverEntry[] {
        return [];
      }
    }
    (
      globalThis as { IntersectionObserver?: unknown }
    ).IntersectionObserver = FakeIO;
    fakeIO.restore = () => {
      (
        globalThis as {
          IntersectionObserver?: typeof IntersectionObserver | undefined;
        }
      ).IntersectionObserver = original;
    };
    try {
      // Render with the timeline expanded so it actually paints dots —
      // a collapsed timeline is a single button with no
      // `data-message-uuid`. Use the small-dot-clustering shape so the
      // timeline produces both a regular dot AND a cluster (both
      // carrying `data-message-uuid`).
      window.localStorage.setItem(timelineExpandedKey(), 'true');
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeMessage(1, 1, 't1', {
              role: 'assistant',
              content: [
                { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
              ],
              created_at: '2026-01-01T00:00:30Z',
            }),
            makeMessage(1, 2, 't2', {
              role: 'assistant',
              content: [
                { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
              ],
              created_at: '2026-01-01T00:00:40Z',
            }),
            makeUserText(1, 3, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
      ]);
      // Custom render where the TIMELINE LIVES INSIDE the conversation
      // body — mirroring TranscriptPane's sticky topRegion layout. The
      // existing `renderOverlay` puts them as siblings, which would
      // mask the bug because the timeline marks live outside the
      // observed container.
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
      vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
        async (threadId) => ({
          messages: messages.get(threadId as number) ?? [],
        }),
      );
      const bodyRef = createRef<HTMLDivElement>();
      render(
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={apiClient}>
            <div ref={bodyRef} data-testid="conversation-body">
              {/* Timeline sits FIRST inside the body, just like the
                  sticky topRegion does in TranscriptPane. */}
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
              <article data-message-uuid="msg-a">msg-a</article>
              <article data-message-uuid="t1">t1</article>
              <article data-message-uuid="t2">t2</article>
              <article data-message-uuid="msg-b">msg-b</article>
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      await screen.findAllByTestId('thread-timeline-dot');
      // The effect re-runs as the messages query settles; the live
      // observer is the most recent.
      expect(fakeIO.instances.length).toBeGreaterThan(0);
      const io = fakeIO.instances[fakeIO.instances.length - 1];
      // Every observed element must be an `<article>`. If the
      // observer was still using a bare `[data-message-uuid]` selector,
      // the timeline dots and the cluster span (both inside the
      // conversation body now) would have crept into `io.observed`.
      for (const el of io.observed) {
        expect(el.tagName.toLowerCase()).toBe('article');
      }
      // And exactly the four articles are observed (one observer per
      // unique element).
      expect(io.observed.size).toBe(4);
    } finally {
      fakeIO.restore();
    }
  });
});
