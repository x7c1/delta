/**
 * External active-thread changes meeting the overlay's own jumps: scroll
 * catch-up, late-loading lanes, and in-flight cross-lane jumps.
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
} from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { Message } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { ThreadTimelineOverlay } from './ThreadTimelineOverlay';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
import { SCROLL_DOM_READY_TIMEOUT_MS } from './timelineScroll';
import {
  makeMessage,
  makeThread,
  makeUserText,
  playheadLeftPx,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay external active-thread change', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('triggers horizontal scroll catch-up so the playhead lands inside the axis viewport after the external switch', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Pick widely-separated timestamps so the global x map keeps msg-a at
    // x=0 and msg-c at the right end (x=240). Lane 2's latest large is
    // msg-c — far to the right of the initial viewport.
    const messages = new Map([
      [
        1,
        [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')],
      ],
      [
        2,
        [
          makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(2, 1, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
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
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
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
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    // Make the wrapper narrow so the scroll-follow effect actually runs.
    Object.defineProperty(wrapper, 'clientWidth', {
      configurable: true,
      get: () => 100,
    });
    // Spy on the smooth-scroll API. The fix routes the catch-up through
    // `scrollTo({ behavior: 'smooth' })` (gated on `userActedTick`), so
    // the external active-thread switch must invoke it exactly like a
    // wheel/click jump would.
    const scrollToMock = vi.fn();
    wrapper.scrollTo = scrollToMock as typeof wrapper.scrollTo;
    // Pre-position the scroll at the left edge — msg-c at x=240 (axis-
    // local) sits well outside [0, 100].
    wrapper.scrollLeft = 0;
    // Flip activeThreadId to lane 2 with the new lane's article in the DOM.
    rerender(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-b">msg-b</article>
              <article data-message-uuid="msg-c">msg-c</article>
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
    // The horizontal scroll-follow effect must fire and request a smooth
    // re-centre — that is the user-visible "playhead becomes visible"
    // half of the fix. The exact left value depends on the live label
    // offset (jsdom returns 0 for `offsetLeft` without explicit CSS), so
    // just assert the call happened with the smooth API.
    await waitFor(() => {
      expect(scrollToMock).toHaveBeenCalled();
    });
    const lastCall = scrollToMock.mock.calls[scrollToMock.mock.calls.length - 1];
    expect(lastCall[0]).toMatchObject({ behavior: 'smooth' });
    expect(typeof (lastCall[0] as ScrollToOptions).left).toBe('number');
  });

  it('leaves the playhead alone when the new lane has no large messages yet', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 2 has a non-text message only (e.g. a tool call placeholder
    // before any large turn lands). The empty-content row is treated as a
    // small mark, not a large one — and {@link buildLargeSortedMessages}
    // includes only large rows, so the new effect must find no candidate
    // and leave the playhead at its current position.
    const messages = new Map([
      [
        1,
        [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')],
      ],
      [
        2,
        [
          // makeMessage's default content is `[]` — that produces a small
          // (auxiliary) mark, NOT a large one.
          makeMessage(2, 0, 'msg-b-small', {
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
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
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
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
    await screen.findAllByTestId('thread-timeline-dot');
    const before = playheadLeftPx(
      screen.getAllByTestId('thread-timeline-playhead')[0],
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body" />
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={2}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    // Give the effect a chance to run; nothing should change.
    await Promise.resolve();
    await Promise.resolve();
    const after = playheadLeftPx(
      screen.getAllByTestId('thread-timeline-playhead')[0],
    );
    expect(after).toBe(before);
  });

  it('follows a later external activeThreadId change even after a cross-lane jump whose target never rendered timed out (no counter latch)', async () => {
    // Regression for the primary latch bug: a cross-lane jump to a uuid that
    // never renders (e.g. an axis click landing on a renders-nothing carrier
    // message) polls to SCROLL_DOM_READY_TIMEOUT_MS. Before the fix the
    // timeout leg returned without releasing the in-flight counter, latching
    // it above zero forever — every subsequent navigator selection was
    // silently swallowed by the external-thread effect's `counter > 0` bail.
    // After the fix the timeout settles and releases the counter, so a later
    // navigator pick still repositions the playhead.
    stubAxisRect({ left: 0, width: 240 });
    // Drive rAF + performance.now so the DOM-ready poll can be pushed past
    // the timeout deterministically.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    const originalPerfNow = window.performance.now;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised: the jump times out rather than being cancelled */
    }) as typeof window.cancelAnimationFrame;
    let nowMs = 5_000;
    window.performance.now = (() => nowMs) as typeof performance.now;
    try {
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
      // msg-a (lane 1, x=0), msg-b (lane 2, x=120), msg-c (lane 3, x=240 tail).
      const messages = new Map<number, Message[]>([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z')]],
        [3, [makeUserText(3, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
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
      const tree = (activeThreadId: number, articleUuids: string[]) => (
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={apiClient}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                {articleUuids.map((uuid) => (
                  <article key={uuid} data-message-uuid={uuid}>
                    {uuid}
                  </article>
                ))}
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={activeThreadId}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>
      );
      // Mount active on lane 3; the playhead lands on the global tail msg-c.
      const { rerender } = render(tree(3, ['msg-c']));
      await screen.findAllByTestId('thread-timeline-dot');
      const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
      // Click the axis at x=0 → nearest is msg-a (lane 1): a cross-lane jump
      // whose target article is deliberately NOT in the DOM, so the poll can
      // never resolve and must time out.
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new MouseEvent('click', {
            clientX: LANE_LEFT_PAD_PX,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // Mirror the overlay-driven switch as a prop change (the jump echoing
      // back). This is skipped by the external effect (own jump in flight).
      rerender(tree(1, ['msg-c']));
      // Drive the DOM-ready poll past the timeout: first drain at elapsed 0,
      // then advance past SCROLL_DOM_READY_TIMEOUT_MS and drain again so the
      // poll hits its timeout branch and settles (releasing the counter).
      act(() => {
        const first = rafCallbacks.splice(0, rafCallbacks.length);
        for (const cb of first) {
          cb(nowMs);
        }
      });
      nowMs = 5_000 + SCROLL_DOM_READY_TIMEOUT_MS + 1;
      act(() => {
        const second = rafCallbacks.splice(0, rafCallbacks.length);
        for (const cb of second) {
          cb(nowMs);
        }
      });
      // Now a genuine external navigator pick lands on lane 2. With the
      // counter released, the external-thread effect repositions the playhead
      // onto lane 2's latest large turn (msg-b, x=120). Before the fix the
      // latched counter would swallow this and the playhead would stay on
      // msg-a (x=0).
      rerender(tree(2, ['msg-b']));
      await waitFor(() => {
        expect(playheadLeftPx(playheads()[1])).toBe(
          `${LANE_LEFT_PAD_PX + 120}px`,
        );
      });
      const lanes = screen.getAllByTestId('thread-timeline-lane');
      expect(lanes[1]).toHaveAttribute('data-active', 'true');
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('repositions the playhead once the new lane’s timeline messages load, when the external change arrived before they were available', async () => {
    // Root cause 2: the external-thread change ref was consumed before the
    // "lane has a large message" check. If the lane's timeline messages had
    // not loaded at click time, the effect bailed AND consumed the ref, so
    // the promised re-fire on the `largeSortedMessages` dep did nothing —
    // the playhead never moved onto the new lane. The fix defers the consume
    // until a reposition actually commits, so the load-triggered re-fire
    // retries.
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 1 holds msg-a (x=0) and the GLOBAL tail msg-d (x=240). Lane 2's
    // messages are withheld initially (empty), then filled with msg-c (x=80,
    // a NON-tail turn) and refetched to mimic a lane whose timeline marks
    // land after the switch. Keeping lane 2's target off the global tail is
    // what isolates this from the auto-anchor effect, which would re-anchor
    // onto the global tail (msg-d) on its own — only the external-thread
    // effect's deferred-consume retry can land the playhead on msg-c.
    let lane2Loaded = false;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => {
        if (threadId === 2) {
          return {
            messages: lane2Loaded
              ? [makeUserText(2, 0, 'msg-c', '2026-01-01T00:01:00Z')]
              : [],
          };
        }
        return {
          messages: [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-d', '2026-01-01T00:03:00Z'),
          ],
        };
      },
    );
    const bodyRef = createRef<HTMLDivElement>();
    const tree = (activeThreadId: number, articleUuids: string[]) => (
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              {articleUuids.map((uuid) => (
                <article key={uuid} data-message-uuid={uuid}>
                  {uuid}
                </article>
              ))}
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={activeThreadId}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>
    );
    const { rerender } = render(tree(1, ['msg-a', 'msg-d']));
    await screen.findAllByTestId('thread-timeline-dot');
    const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
    // The auto-anchor lands on the active lane’s latest large turn (msg-d,
    // x=240 — also the global tail here).
    await waitForPlayheadAt(`${LANE_LEFT_PAD_PX + 240}px`);
    // Switch to lane 2 externally while its timeline messages are still
    // absent — the effect must bail without consuming the change.
    rerender(tree(2, []));
    await Promise.resolve();
    await Promise.resolve();
    // Now lane 2's messages land: fill the mock and refetch so
    // `largeSortedMessages` updates and the external effect re-fires.
    lane2Loaded = true;
    await act(async () => {
      await queryClient.refetchQueries();
    });
    // The playhead must now move onto lane 2's latest large turn (msg-c,
    // x=80) — NOT the global tail msg-d (x=240) the auto-anchor would pick.
    // Before the fix the consumed change ref short-circuited the retry and
    // the playhead stayed on msg-d.
    await waitFor(() => {
      expect(playheadLeftPx(playheads()[1])).toBe(`${LANE_LEFT_PAD_PX + 80}px`);
    });
    const lanes = screen.getAllByTestId('thread-timeline-lane');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
  });

  it('lets a newer external activeThreadId change cancel an in-flight cross-lane jump to a different thread and win', async () => {
    // Newest user intent wins: while a cross-lane jump to lane 2 is still
    // polling for its target, an external navigator pick of lane 3 must
    // cancel the stale jump and reposition the playhead onto lane 3. Before
    // the fix the external effect bailed on `counter > 0`, so the playhead
    // stayed on the superseded lane-2 target.
    stubAxisRect({ left: 0, width: 240 });
    // Capture rAF (never drive it) so the cross-lane jump's DOM-ready poll
    // stays pending — the jump remains in flight until it is cancelled.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* the pending poll is abandoned when the jump is cancelled */
    }) as typeof window.cancelAnimationFrame;
    try {
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
      // msg-a (lane 1, x=0), msg-b (lane 2, x=120), msg-c (lane 3, x=240 tail).
      const messages = new Map<number, Message[]>([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z')]],
        [3, [makeUserText(3, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
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
      const tree = (activeThreadId: number, articleUuids: string[]) => (
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={apiClient}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                {articleUuids.map((uuid) => (
                  <article key={uuid} data-message-uuid={uuid}>
                    {uuid}
                  </article>
                ))}
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={activeThreadId}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>
      );
      const { rerender } = render(tree(1, ['msg-a']));
      await screen.findAllByTestId('thread-timeline-dot');
      const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
      // Click x=120 → nearest is msg-b (lane 2): a cross-lane jump whose
      // target is not in the DOM, so it stays in flight (poll captured, never
      // driven).
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new MouseEvent('click', {
            clientX: LANE_LEFT_PAD_PX + 120,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      // Sanity: the playhead moved onto the jump target msg-b (x=120).
      expect(playheadLeftPx(playheads()[1])).toBe(`${LANE_LEFT_PAD_PX + 120}px`);
      // A newer external navigator pick lands on lane 3 (different from the
      // in-flight jump's lane-2 target). The stale jump must be cancelled and
      // the playhead must move onto lane 3's latest large turn (msg-c, x=240).
      rerender(tree(3, ['msg-c']));
      await waitFor(() => {
        expect(playheadLeftPx(playheads()[2])).toBe(
          `${LANE_LEFT_PAD_PX + 240}px`,
        );
      });
      const lanes = screen.getAllByTestId('thread-timeline-lane');
      expect(lanes[2]).toHaveAttribute('data-active', 'true');
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('still skips the overlay’s own cross-lane jump echoing back as a prop change (no snap to the lane tail)', async () => {
    // The complement of the cancel-and-win case: when the prop change IS the
    // overlay's own jump echoing back (its recorded target equals the new
    // activeThreadId), the external effect must keep skipping — otherwise it
    // would override the user's picked message with the lane's latest large
    // turn (the tail).
    stubAxisRect({ left: 0, width: 240 });
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* poll stays pending; the jump is neither cancelled nor completed */
    }) as typeof window.cancelAnimationFrame;
    try {
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      // Lane 2 carries a non-tail turn msg-b (x=120) and the tail msg-c
      // (x=240). The jump targets msg-b; a wrongly-firing effect would snap
      // to msg-c.
      const messages = new Map<number, Message[]>([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [
          2,
          [
            makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z'),
            makeUserText(2, 1, 'msg-c', '2026-01-01T00:02:00Z'),
          ],
        ],
      ]);
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
      const tree = (activeThreadId: number, articleUuids: string[]) => (
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={apiClient}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                {articleUuids.map((uuid) => (
                  <article key={uuid} data-message-uuid={uuid}>
                    {uuid}
                  </article>
                ))}
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={activeThreadId}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>
      );
      // Mount active on lane 1; the playhead lands on the global tail msg-c.
      const { rerender } = render(tree(1, ['msg-a']));
      await screen.findAllByTestId('thread-timeline-dot');
      const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
      // Click x=120 → nearest is the non-tail lane-2 turn msg-b. Cross-lane
      // jump to lane 2, target msg-b; poll captured, never driven (in flight).
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new MouseEvent('click', {
            clientX: LANE_LEFT_PAD_PX + 120,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      const targetX = playheadLeftPx(playheads()[1]);
      // The jump landed on msg-b (x=120), NOT the lane-2 tail msg-c (x=240).
      expect(targetX).toBe(`${LANE_LEFT_PAD_PX + 120}px`);
      // The prop now flips to lane 2 — the overlay's own jump echoing back.
      // The external effect must skip it and leave the playhead on msg-b.
      rerender(tree(2, ['msg-b', 'msg-c']));
      await Promise.resolve();
      await Promise.resolve();
      expect(playheadLeftPx(playheads()[1])).toBe(targetX);
      expect(playheadLeftPx(playheads()[1])).not.toBe(
        `${LANE_LEFT_PAD_PX + 240}px`,
      );
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });
});
