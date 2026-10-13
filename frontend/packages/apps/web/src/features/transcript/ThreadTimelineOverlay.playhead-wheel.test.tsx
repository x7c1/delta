/**
 * Wheel stepping of the playhead: single steps, the clamps at either end,
 * and stepping across lanes.
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
import { WHEEL_VELOCITY_WINDOW_MS } from './timelineWheel';
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

describe('ThreadTimelineOverlay playhead', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('advances exactly one step on a single slow full-notch wheel event', async () => {
    // Regression for v9: the v9 staircase tripped the 2-step bucket at
    // exactly one notch's worth of |delta| (100 px), so the user could
    // never land on the immediate prev/next message — a slow single
    // mouse-wheel notch always jumped two messages. The fix lifts the
    // first acceleration bucket above the per-event clamp, so a single
    // notch of any size up to the clamp always walks exactly one step.
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
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
      // Initial playhead anchors to the active lane’s latest large turn
      // (msg-c, x=1 → 240px).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
      // A single full-notch wheel-up event (|deltaY| = 100, the canonical
      // mouse-wheel notch on Linux/Chrome) lands the playhead on the
      // immediate previous large turn (msg-b at x=0.5 → 120px), NOT msg-a.
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -100,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('advances one step on a leisurely sub-notch wheel event and suppresses page scroll', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Three evenly-spaced "large" turns: x=0, x=0.5, x=1 (px 0, 120, 240).
    // The wheel step navigates the main-conversation subset, so each message
    // must carry an authored text block to land on the large list.
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
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }, { uuid: 'msg-c' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead anchors to the active lane’s latest large turn
    // (msg-c, x=1 → 240px).
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    // A single sub-notch wheel-up event (cumulative |delta| under the
    // first staircase threshold) lands in the slowest bucket → exactly
    // one step back. preventDefault is also called so the page scroll
    // does not run alongside the navigation step.
    const body = screen.getByTestId('thread-timeline-axis-column');
    const wheel = new WheelEvent('wheel', {
      deltaY: -50,
      bubbles: true,
      cancelable: true,
    });
    const preventDefault = vi.spyOn(wheel, 'preventDefault');
    act(() => {
      body.dispatchEvent(wheel);
    });
    expect(preventDefault).toHaveBeenCalled();
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
  });

  it('clamps at the newest end: a wheel-down at the last message does not advance', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Use large turns so each is a wheel-step target — the wheel handler
    // walks the main-conversation subset, and empty-content user dots are
    // small (and now cluster) which would put them outside the subset.
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
    // Initial position is the last message (msg-b, x=1 → 240px).
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const body = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: 100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    // Still at msg-b — the clamp blocks any further advance.
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('clamps at the oldest end: a wheel-up at the first message does not retreat', async () => {
    // Drive a virtual clock so the rolling-window accumulator can be reset
    // between events without sleeping the test.
    let nowMs = 1_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      // Both messages must be "large" so they survive the wheel-step subset
      // filter — the wheel only walks main-conversation turns.
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
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Step back once (msg-b → msg-a).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -100,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
      // Advance past the rolling window so the accumulator resets — the
      // next event is treated as a fresh leisurely turn rather than a
      // continuation of the previous burst.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 1;
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -100,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Still at msg-a — the clamp blocks any further retreat.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it("crosses lanes when wheel-stepping from lane A's last message into lane B's first", async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* tests do not exercise cancellation here */
    }) as typeof window.cancelAnimationFrame;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Lane 1 holds msg-a then msg-b; lane 2 holds msg-c (later). The
      // global sorted list is [msg-a, msg-b, msg-c], so wheel-up from the
      // final position (msg-c) lands on msg-b, which lives on lane 1 — a
      // cross-lane step from the user's starting subthread (lane 2).
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      // All three messages are "large" so the wheel-step subset includes
      // them and the cross-lane walk steps msg-c → msg-b.
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
        [
          2,
          [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')],
        ],
      ]);
      // Start with lane 2 active; only msg-c is in the conversation pane.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial settle does not fire; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Wheel-up from msg-c → msg-b. msg-b lives on lane 1 (cross-lane).
      // A sub-notch event keeps the staircase at one step so the
      // assertion targets msg-b (the immediate large neighbour), not
      // msg-a (two steps back, which a 100-px notch would reach).
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
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // The scroll is deferred to the next frame; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Re-render with lane 1 active and its article in the pane,
      // mirroring the live app's response to the thread switch.
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
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(performance.now());
      }
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
      expect(scrollIntoView).toHaveBeenCalledWith({ block: 'start' });
      const target = within(screen.getByTestId('conversation-body')).getByText(
        'msg-b',
      );
      expect(scrollIntoView.mock.instances[0]).toBe(target);
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });
});
