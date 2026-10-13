/**
 * Reactions to an externally driven active-thread change
 * (Navigator click, breadcrumb, etc.): the reposition and the auto-anchor.
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

describe('ThreadTimelineOverlay does not override an external active-thread change', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('lets a Navigator-driven setActiveThread stick when the message-list reference changes underneath', async () => {
    // Regression: v2-v4 had the timeline re-fire its auto-switch effect on
    // any `activeMessage` reference change (a background message-list
    // refetch landing right after a Navigator click was the common
    // trigger), which then overwrote the Navigator's chosen thread.
    //
    // The fix snapshots the active message into a ref and depends on
    // `scrubTick` alone, so only a deliberate scrub re-fires the effect.
    // This test simulates the sequence: scrub to land the playhead on
    // thread 1, then a Navigator click flips active thread to 2, then a
    // re-render with a fresh messages map (the refetch) — the active
    // thread must remain 2.
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    const buildMessages = () =>
      new Map<number, Message[]>([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
        [2, [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
    // Start with lane 2 active (the latest message lands there).
    const { rerender, bodyRef } = renderOverlay({
      threads,
      messagesByThread: buildMessages(),
      activeThreadId: 2,
      conversationArticles: [{ uuid: 'msg-c' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Scrub: wheel-up from msg-c → msg-b lands the playhead on thread 1.
    // (Both Navigator and the timeline would each call setActiveThread,
    // so confirm the timeline DID flip the store to thread 1 first.)
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
    await waitFor(() => {
      expect(useNavStore.getState().activeThreadId).toBe(1);
    });
    // Now a Navigator click flips the store back to thread 2.
    act(() => {
      useNavStore.getState().setActiveThread(2);
    });
    expect(useNavStore.getState().activeThreadId).toBe(2);
    // Simulate the post-click refetch: a fresh messages map (new array
    // identities) lands, the overlay re-renders. Without the fix, the
    // active-message effect would re-fire and call setActiveThread(1)
    // because the playhead is still on msg-b. With the fix, scrubTick
    // did not change, so the effect stays inert.
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    const fresh = buildMessages();
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => ({
        messages: fresh.get(threadId as number) ?? [],
      }),
    );
    rerender(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
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
    // Give microtasks + any effect ticks a chance to run.
    await Promise.resolve();
    await Promise.resolve();
    // The Navigator's choice (thread 2) must win — the timeline must not
    // have overridden it back to thread 1.
    expect(useNavStore.getState().activeThreadId).toBe(2);
  });
});

describe('ThreadTimelineOverlay external active-thread change', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  /**
   * When `activeThreadId` flips because the user picked a subthread from
   * outside the overlay (Navigator click, breadcrumb, etc.) the playhead
   * must move to the new lane's latest main-conversation turn AND the
   * timeline must horizontally scroll so that new x is on screen. Without
   * the fix the playhead stayed pointed at the previous lane's message,
   * and on long sessions the playhead's x sat outside the axis viewport —
   * invisible to the user.
   */
  it('moves the playhead to the latest large message of the new lane on external activeThreadId change', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 1 carries msg-a at t=0; lane 2 carries msg-b at t=1m and
    // msg-c at t=2m. The latest large message in lane 2 is msg-c.
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
    // Mount with lane 1 as the active thread (the anchor rationale is
    // asserted at the sanity check below).
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
    // Both lanes' queries have to have landed before the external switch
    // means anything — one mark per message across the two lanes.
    await waitFor(() => {
      expect(screen.getAllByTestId('thread-timeline-dot')).toHaveLength(3);
    });
    // Sanity: the initial playhead sits on lane 1's latest large turn
    // (msg-a at x=0 = LANE_LEFT_PAD_PX). The mount anchors to the ACTIVE
    // lane, not whichever lane holds the global tail.
    const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
    await waitForPlayheadAt(`${LANE_LEFT_PAD_PX}px`);
    // Now flip activeThreadId to lane 2 from the outside, mirroring a
    // Navigator click. Re-render the pane with lane 2's articles so the
    // DOM matches what the live app shows after the switch.
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
    // The effect picks the latest large message in the new lane (msg-c).
    // msg-c sits at the global tail (x = 240 inside the axis), so the
    // playhead's translateX must be LANE_LEFT_PAD_PX + 240. The lane-2
    // playhead — not lane-1's — is the one that should track this x.
    await waitFor(() => {
      const lane2Playhead = playheads()[1];
      expect(playheadLeftPx(lane2Playhead)).toBe(
        `${LANE_LEFT_PAD_PX + 240}px`,
      );
    });
    // The lane highlight follows the new active message's lane (lane 2).
    const lanes = screen.getAllByTestId('thread-timeline-lane');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
  });

  /**
   * WorkspaceScreen's binding flush invalidates the active thread's messages
   * on EVERY selection, so a refetch — and with it a brand-new
   * `sortedMessages` array identity — is guaranteed to land moments after an
   * external reposition commits. The playhead must stay on the reposition
   * target: the old index-canonical implementation "realigned" the index from
   * a ref that could lag one commit behind the reposition, so the refetch
   * could revert the playhead onto the PREVIOUS thread's message. With the
   * UUID as canonical state and the index derived per render, an array
   * identity change cannot move the playhead by construction.
   */
  it('keeps the playhead on the external reposition target when a messages refetch replaces the sorted array identity', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 1: msg-a at t=0 (plus msg-e at t=30s once the refetch lands).
    // Lane 2: msg-b at t=1m, msg-c at t=2m (the reposition target). msg-e's
    // timestamp sits INSIDE the existing time range so the refetch changes
    // the array contents/identity without moving msg-c's x (240).
    let lane1Grew = false;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => {
        if (threadId === 1) {
          return {
            messages: [
              makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
              ...(lane1Grew
                ? [makeUserText(1, 1, 'msg-e', '2026-01-01T00:00:30Z')]
                : []),
            ],
          };
        }
        return {
          messages: [
            makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z'),
            makeUserText(2, 1, 'msg-c', '2026-01-01T00:02:00Z'),
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
    const { rerender } = render(tree(1, ['msg-a']));
    // Both lanes' queries have to have landed before the external switch
    // means anything — one mark per message across the two lanes.
    await waitFor(() => {
      expect(screen.getAllByTestId('thread-timeline-dot')).toHaveLength(3);
    });
    const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
    // Mount anchor: lane 1's latest large turn (msg-a at x=0).
    await waitForPlayheadAt(`${LANE_LEFT_PAD_PX}px`);
    // External selection of lane 2: the reposition commits onto msg-c.
    rerender(tree(2, ['msg-b', 'msg-c']));
    await waitFor(() => {
      expect(playheadLeftPx(playheads()[1])).toBe(`${LANE_LEFT_PAD_PX + 240}px`);
    });
    // The post-selection refetch lands: lane 1 grew, so the refetched
    // `sortedMessages` is a superset with a new array identity.
    lane1Grew = true;
    await act(async () => {
      await queryClient.refetchQueries();
    });
    // The playhead must still sit on msg-c — not revert to a pre-selection
    // message.
    expect(playheadLeftPx(playheads()[1])).toBe(`${LANE_LEFT_PAD_PX + 240}px`);
    const lanes = screen.getAllByTestId('thread-timeline-lane');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
  });

  /**
   * The same identity-change guarantee for a USER pick (wheel step): a message
   * appended to a NON-active lane replaces the sorted array, and the playhead
   * must not move off the picked message.
   */
  it('keeps the playhead on the user-picked message when a refetch appends messages to a non-active lane', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 1 (active): msg-a at t=0, msg-b at t=1m. Lane 2: msg-c at t=2m,
    // growing by msg-d at t=1m30s — inside the range, so lane 1's xs hold.
    let lane2Grew = false;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => {
        if (threadId === 1) {
          return {
            messages: [
              makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
              makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
            ],
          };
        }
        return {
          messages: [
            makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z'),
            ...(lane2Grew
              ? [makeUserText(2, 1, 'msg-d', '2026-01-01T00:01:30Z')]
              : []),
          ],
        };
      },
    );
    const bodyRef = createRef<HTMLDivElement>();
    render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
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
    await screen.findAllByTestId('thread-timeline-dot');
    const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
    // Mount anchor: lane 1's latest large turn (msg-b at x=120).
    await waitForPlayheadAt(`${LANE_LEFT_PAD_PX + 120}px`);
    // Wheel-up one step: the user picks msg-a (x=0), a same-lane jump.
    const body = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
      );
    });
    await waitFor(() => {
      expect(playheadLeftPx(playheads()[0])).toBe(`${LANE_LEFT_PAD_PX}px`);
    });
    // Lane 2 grows and its refetch replaces the sorted array identity.
    lane2Grew = true;
    await act(async () => {
      await queryClient.refetchQueries();
    });
    // The playhead stays on msg-a.
    expect(playheadLeftPx(playheads()[0])).toBe(`${LANE_LEFT_PAD_PX}px`);
  });

  /**
   * Mount-anchor contract (the remount corner of the same user-visible bug): a
   * fresh overlay mount with a non-null `activeThreadId` must anchor to THAT
   * thread's latest large turn — never to another lane's global tail — and
   * must keep waiting (no wrong-lane flash) when the lane's messages have not
   * loaded yet, anchoring as soon as they land.
   */
  it('anchors a fresh mount to the active thread’s latest large turn once its messages load, without flashing the global tail', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 2 holds the global tail (msg-c at t=2m). Lane 1 (the ACTIVE lane)
    // carries msg-a at t=0 and msg-b at t=1m, withheld until `lane1Loaded`.
    let lane1Loaded = false;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => {
        if (threadId === 1) {
          return {
            messages: lane1Loaded
              ? [
                  makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
                  makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
                ]
              : [],
          };
        }
        return { messages: [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')] };
      },
    );
    const bodyRef = createRef<HTMLDivElement>();
    render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body" />
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
    const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
    // Lane 1's messages are still absent: the playhead must NOT sit on the
    // global tail (msg-c at x=240) — the old mount behavior that briefly
    // highlighted the wrong lane on every fresh expand.
    expect(playheadLeftPx(playheads()[0])).not.toBe(
      `${LANE_LEFT_PAD_PX + 240}px`,
    );
    // Lane 1's messages land: the anchor retries via the
    // `largeSortedMessages` dep and lands on the lane's latest large turn
    // (msg-b at x=120).
    lane1Loaded = true;
    await act(async () => {
      await queryClient.refetchQueries();
    });
    await waitFor(() => {
      expect(playheadLeftPx(playheads()[0])).toBe(`${LANE_LEFT_PAD_PX + 120}px`);
    });
    const lanes = screen.getAllByTestId('thread-timeline-lane');
    expect(lanes[0]).toHaveAttribute('data-active', 'true');
  });

  it('anchors a fresh mount to the global tail only when activeThreadId is null', async () => {
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
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: null,
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // No active thread to anchor onto → the global tail (msg-b at x=240).
    await waitFor(() => {
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX + 240}px`);
    });
  });
});
