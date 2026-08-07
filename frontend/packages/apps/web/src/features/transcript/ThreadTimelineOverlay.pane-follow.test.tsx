/**
 * Pane scroll -> playhead follow, including reserve-line
 * follower selection.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import {
  act,
  fireEvent,
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
import { useNavStore } from '../../store/navStore';
import {
  LANE_LEFT_PAD_PX,
  PANE_SCROLL_DEBOUNCE_MS,
  PANE_SCROLL_OBSERVER_THRESHOLD,
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
} from './ThreadTimelineOverlay';
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

describe('ThreadTimelineOverlay pane scroll → playhead follow', () => {
  // Pane-scroll → playhead is the bidirectional half of the sync: when
  // the user manually scrolls the conversation pane, an IntersectionObserver
  // on each message article picks the topmost-visible message and drives
  // the playhead to it WITHOUT bumping `scrubTick` (so no thread switch,
  // no scrollIntoView, no ping-pong). These tests exercise the wiring,
  // the no-recursion guarantee, and the programmatic-scroll guard.

  /**
   * A handle on the IntersectionObserver instances the overlay creates so
   * a test can synthesize entries directly: jsdom does not run a real
   * layout / viewport, so we cannot rely on actual scroll positions to
   * trigger callbacks.
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
        // Attach `emit` here so every instance has it the moment it
        // lands in `instances` — earlier "post-construction patch"
        // approaches missed the first push.
        this.emit = (entries) => {
          this.callback(
            entries as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        };
        instances.push(this as unknown as FakeIO);
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
    ).IntersectionObserver = FakeIntersectionObserver;
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

  it('uses threshold=PANE_SCROLL_OBSERVER_THRESHOLD and observes every rendered message article', async () => {
    const fake = installFakeIO();
    try {
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
          ],
        ],
      ]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [
          { uuid: 'msg-a' },
          { uuid: 'msg-b' },
          { uuid: 'msg-c' },
        ],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // The effect re-runs when `sortedMessages` settles (async query
      // arrival), so multiple FakeIO instances accumulate; the LIVE one
      // is the most recent. Earlier instances were disconnected by the
      // effect's cleanup.
      expect(fake.instances.length).toBeGreaterThan(0);
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      // Re-read the live observer on every poll rather than asserting against
      // a snapshot: a re-run of the effect between the capture and the
      // assertion disconnects the captured instance (clearing its `observed`
      // set) and installs a fresh one, so a snapshot can go empty under the
      // test's feet. The claims are unchanged — the live observer carries the
      // production threshold and observes every article in the body.
      await waitFor(() => {
        const io = fake.instances.at(-1);
        expect(io?.options?.threshold).toBe(PANE_SCROLL_OBSERVER_THRESHOLD);
        for (const a of articles) {
          expect(io?.observed.has(a)).toBe(true);
        }
      });
    } finally {
      fake.restore();
    }
  });

  it('moves the playhead to the topmost-visible article on pane scroll, without bumping scrubTick', async () => {
    const fake = installFakeIO();
    const setActiveThreadSpy = vi.spyOn(
      useNavStore.getState(),
      'setActiveThread',
    );
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
          ],
        ],
      ]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [
          { uuid: 'msg-a' },
          { uuid: 'msg-b' },
          { uuid: 'msg-c' },
        ],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial playhead sits on the last message (msg-c at x=240).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      // Simulate the user scrolling up so msg-a is closest to the
      // viewport top (smallest boundingClientRect.top) and msg-b is
      // partially visible below it; msg-c is now off-screen.
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: articles[2],
              isIntersecting: false,
              boundingClientRect: { top: 9999 } as DOMRect,
            },
            {
              target: articles[1],
              isIntersecting: true,
              boundingClientRect: { top: 120 } as DOMRect,
            },
            {
              target: articles[0],
              isIntersecting: true,
              boundingClientRect: { top: 10 } as DOMRect,
            },
          ]);
        });
        // Debounce: advance past PANE_SCROLL_DEBOUNCE_MS so the flush
        // fires. Wrapped in `act` separately so React commits the
        // resulting state change.
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Playhead snapped to msg-a's x (0) — the topmost-visible message
      // wins.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
      // CRUCIAL: pane-scroll updates must NOT trigger an active-thread
      // switch (the pane is already inside the active subthread).
      expect(setActiveThreadSpy).not.toHaveBeenCalled();
    } finally {
      fake.restore();
      setActiveThreadSpy.mockRestore();
    }
  });

  it('suppresses pane-scroll updates fired within the programmatic-scroll guard window', async () => {
    // The classic ping-pong: a timeline → pane jump triggers
    // scrollIntoView, which fires IO entries, which would re-update the
    // playhead. The guard window after a programmatic scroll blocks that
    // feedback. This test fires a click on the timeline (programmatic
    // scroll), then immediately emits IO entries for a DIFFERENT
    // message; the playhead must STAY at the clicked target, not jump
    // to the IO-reported one.
    const fake = installFakeIO();
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
      ]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // "Inside the guard window" has to be a fact the test controls, not a
      // bet on how much wall-clock time the steps below consume: the guard is
      // `performance.now() - lastProgrammaticScrollAt < 200 ms`, and the
      // awaits between the click and the emit are unbounded on a loaded
      // machine. Freeze the clock (the same idiom the wheel-cadence tests use)
      // so the emit provably lands inside the window.
      const nowSpy = vi
        .spyOn(performance, 'now')
        .mockImplementation(() => 10_000);
      try {
        // Click at x=0: timeline jumps to msg-a (programmatic scroll fires).
        act(() => {
          fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
            clientX: 0,
          });
        });
        // Playhead now at msg-a (x=0).
        await waitFor(() => {
          expect(
            playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
          ).toBe(`${LANE_LEFT_PAD_PX}px`);
        });
        // While inside the guard window, emit an IO entry claiming
        // msg-b is topmost-visible — exactly what the jump's own scroll
        // would produce as it animates past msg-b. The playhead must NOT
        // jump to msg-b.
        const articles = within(
          screen.getByTestId('conversation-body'),
        ).getAllByText(/msg-/);
        const io = await getLiveIO(fake, articles.length);
        vi.useFakeTimers();
        try {
          act(() => {
            io.emit([
              {
                target: articles[1],
                isIntersecting: true,
                boundingClientRect: { top: 5 } as DOMRect,
              },
            ]);
            // Debounce shorter than the guard window: still inside guard.
            vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
          });
        } finally {
          vi.useRealTimers();
        }
      } finally {
        nowSpy.mockRestore();
      }
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
      // Sanity: the guard exists as a constant the production code reads.
      expect(PANE_SCROLL_PROGRAMMATIC_GUARD_MS).toBeGreaterThan(
        PANE_SCROLL_DEBOUNCE_MS,
      );
    } finally {
      fake.restore();
    }
  });
});

describe('ThreadTimelineOverlay pane scroll follower reserve-line selection', () => {
  // Regression suite for the leftward playhead yank: after a same-lane jump
  // parks the target at the reading-region start line (message articles carry
  // `scroll-margin-top: var(--delta-top-region-reserve)`), the PREVIOUS
  // article is left intersecting by a sliver in the reserve band above the
  // line, with a smaller (more negative) top. The old follower committed the
  // raw smallest-top, so any IO flush that escaped the guards after the jump
  // yanked the playhead one mark backwards. The fix selects the article that
  // OWNS the line (skipping any whose bottom edge has crossed above it),
  // making the post-jump commit idempotent: it resolves to the very message
  // the scroll established, so WHEN the flush fires no longer matters.

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
    ).IntersectionObserver = FakeIntersectionObserver;
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

  /**
   * The top-region reserve (px) installed on the conversation body for these
   * cases. In production `TranscriptPane` drives `--delta-top-region-reserve`
   * inline; here we set it directly so `readTopRegionReserve` resolves it and
   * the follower's reading-region line sits at `containerTop + RESERVE`
   * (containerTop is 0 in jsdom).
   */
  const RESERVE = 40;

  function playheadLeft(): string {
    return playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]);
  }

  function articlesInBody(): HTMLElement[] {
    return within(screen.getByTestId('conversation-body')).getAllByText(/msg-/);
  }

  function threeMessages() {
    return new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
  }

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('leaves the playhead on the scrubbed target when an IO flush lands after the guard window (post-scroll geometry is a no-op)', async () => {
    // Reproduces the leftward yank at b2bcf0b: a same-lane wheel step parks
    // msg-b at the reserve line, msg-a is left partially visible in the
    // reserve band above it, and a late IO flush (past the 200 ms guard)
    // would commit the raw smallest-top (msg-a). With the fix, msg-a is
    // skipped (its bottom edge sits AT the line) and the flush resolves back
    // to msg-b — the message the scroll established.
    const fake = installFakeIO();
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      const { bodyRef } = renderOverlay({
        threads: [makeThread(1)],
        messagesByThread: threeMessages(),
        activeThreadId: 1,
        conversationArticles: [
          { uuid: 'msg-a' },
          { uuid: 'msg-b' },
          { uuid: 'msg-c' },
        ],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      bodyRef.current!.style.setProperty(
        '--delta-top-region-reserve',
        `${RESERVE}px`,
      );
      // Playhead starts on the tail (msg-c, x=240).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

      // Same-lane wheel-up: msg-c → msg-b. This bumps scrubTick, which stamps
      // the programmatic-scroll guard and (in production) parks msg-b at the
      // reserve line via scrollIntoView.
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
      await waitFor(() => {
        expect(playheadLeft()).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
      });

      const articles = articlesInBody();
      const io = await getLiveIO(fake, articles.length);
      const msgA = articles.find((a) => a.textContent === 'msg-a')!;
      const msgB = articles.find((a) => a.textContent === 'msg-b')!;
      const msgC = articles.find((a) => a.textContent === 'msg-c')!;

      // Expire the guard window so the flush is NOT suppressed by time — this
      // is the exact condition an observer re-bind hits mid-session.
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgC,
              isIntersecting: false,
              boundingClientRect: { top: 9999, bottom: 9999 } as DOMRect,
            },
            // msg-a: sliver still visible in the reserve band, smallest top,
            // but its BOTTOM sits AT the reserve line — body has left the
            // reading region.
            {
              target: msgA,
              isIntersecting: true,
              boundingClientRect: { top: -30, bottom: RESERVE } as DOMRect,
            },
            // msg-b: parked exactly at the reserve line by the scroll.
            {
              target: msgB,
              isIntersecting: true,
              boundingClientRect: {
                top: RESERVE,
                bottom: RESERVE + 180,
              } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Idempotent: still on msg-b (x=120). At b2bcf0b this asserted x=0.
      expect(playheadLeft()).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.performance.now = originalPerfNow;
    }
  });

  it('still follows a genuine user scroll that brings an earlier message body below the reserve line (follower not deadened)', async () => {
    // The skip must not deaden legitimate follows: when the user scrolls so an
    // earlier article's body starts just below the reading-region line, the
    // playhead moves to it.
    const fake = installFakeIO();
    stubAxisRect({ left: 0, width: 240 });
    try {
      const { bodyRef } = renderOverlay({
        threads: [makeThread(1)],
        messagesByThread: threeMessages(),
        activeThreadId: 1,
        conversationArticles: [
          { uuid: 'msg-a' },
          { uuid: 'msg-b' },
          { uuid: 'msg-c' },
        ],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      bodyRef.current!.style.setProperty(
        '--delta-top-region-reserve',
        `${RESERVE}px`,
      );
      // Playhead starts on the tail (msg-c, x=240).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

      const articles = articlesInBody();
      const io = await getLiveIO(fake, articles.length);
      const msgA = articles.find((a) => a.textContent === 'msg-a')!;
      const msgB = articles.find((a) => a.textContent === 'msg-b')!;
      const msgC = articles.find((a) => a.textContent === 'msg-c')!;

      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgC,
              isIntersecting: false,
              boundingClientRect: { top: 9999, bottom: 9999 } as DOMRect,
            },
            // msg-a: body starts just BELOW the reserve line and extends down
            // — clearly in the reading region.
            {
              target: msgA,
              isIntersecting: true,
              boundingClientRect: {
                top: RESERVE + 5,
                bottom: RESERVE + 200,
              } as DOMRect,
            },
            {
              target: msgB,
              isIntersecting: true,
              boundingClientRect: {
                top: RESERVE + 205,
                bottom: RESERVE + 400,
              } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Followed the scroll to msg-a (x=0).
      expect(playheadLeft()).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
    }
  });

  it('selects a tall article that spans the reserve line rather than skipping ahead to the next', async () => {
    // A tall article whose top is ABOVE the line and bottom BELOW it still
    // occupies the reading region — it must be selected, not skipped to the
    // next article.
    const fake = installFakeIO();
    stubAxisRect({ left: 0, width: 240 });
    try {
      const { bodyRef } = renderOverlay({
        threads: [makeThread(1)],
        messagesByThread: threeMessages(),
        activeThreadId: 1,
        conversationArticles: [
          { uuid: 'msg-a' },
          { uuid: 'msg-b' },
          { uuid: 'msg-c' },
        ],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      bodyRef.current!.style.setProperty(
        '--delta-top-region-reserve',
        `${RESERVE}px`,
      );

      const articles = articlesInBody();
      const io = await getLiveIO(fake, articles.length);
      const msgA = articles.find((a) => a.textContent === 'msg-a')!;
      const msgB = articles.find((a) => a.textContent === 'msg-b')!;
      const msgC = articles.find((a) => a.textContent === 'msg-c')!;

      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgC,
              isIntersecting: false,
              boundingClientRect: { top: 9999, bottom: 9999 } as DOMRect,
            },
            // msg-a: tall — top well ABOVE the line, bottom well BELOW it. It
            // spans the reading-region start line.
            {
              target: msgA,
              isIntersecting: true,
              boundingClientRect: {
                top: -100,
                bottom: RESERVE + 200,
              } as DOMRect,
            },
            // msg-b: fully below the line (the article that would be picked if
            // the spanning article were wrongly skipped).
            {
              target: msgB,
              isIntersecting: true,
              boundingClientRect: {
                top: RESERVE + 205,
                bottom: RESERVE + 400,
              } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // The spanning article msg-a (x=0) is selected — no skip-ahead to msg-b.
      expect(playheadLeft()).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
    }
  });
});
