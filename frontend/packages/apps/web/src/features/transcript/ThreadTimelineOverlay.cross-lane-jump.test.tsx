/**
 * Cross-lane jump IntersectionObserver in-flight guard and the
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
import { ThreadTimelineOverlay } from './ThreadTimelineOverlay';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
import {
  PANE_SCROLL_DEBOUNCE_MS,
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
} from './useTimelinePaneScrollFollow';
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
import { TIMERS_ONLY } from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay cross-lane jump in-flight guard', () => {
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
      vi.useFakeTimers(TIMERS_ONLY);
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
      vi.useFakeTimers(TIMERS_ONLY);
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
      vi.useFakeTimers(TIMERS_ONLY);
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

describe('ThreadTimelineOverlay article-anchored uuid selector', () => {
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
