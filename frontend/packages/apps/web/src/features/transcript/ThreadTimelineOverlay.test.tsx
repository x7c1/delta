import { createRef } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { Message, Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import {
  ThreadTimelineOverlay,
  TIMELINE_EXPANDED_STORAGE_KEY,
  TIMELINE_JUMP_HIGHLIGHT_CLASS,
  WHEEL_DELTA_LINE_PX,
  WHEEL_PER_EVENT_CLAMP_PX,
  WHEEL_VELOCITY_WINDOW_MS,
  normalizeWheelDeltaPx,
  stepsForCumulativePx,
} from './ThreadTimelineOverlay';

function makeThread(
  id: number,
  overrides: Partial<Thread> = {},
): Thread {
  return {
    id,
    session_id: 'session-1',
    title: `thread ${id}`,
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function makeMessage(
  threadId: number,
  seq: number,
  uuid: string,
  overrides: Partial<Message> = {},
): Message {
  return {
    uuid,
    session_id: 'session-1',
    thread_id: threadId,
    role: 'user',
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq,
    content_text: null,
    content: [],
    created_at: '2026-01-01T00:00:00Z',
    model: null,
    git_branch: null,
    cwd: null,
    response_time_ms: null,
    ...overrides,
  };
}

/**
 * A user-role message carrying a single text block — i.e. a "large"
 * main-conversation turn the wheel step navigation targets. Tests that
 * exercise wheel stepping use this so the messages land in the
 * `largeSortedMessages` subset (the wheel skips auxiliary tool/meta marks).
 */
function makeUserText(
  threadId: number,
  seq: number,
  uuid: string,
  createdAt: string,
): Message {
  return makeMessage(threadId, seq, uuid, {
    role: 'user',
    content: [{ type: 'text', text: `text ${uuid}` }],
    created_at: createdAt,
  });
}

/**
 * Render the overlay against a stubbed ApiClient that resolves
 * `getThreadMessages` from the provided in-memory map. The conversation body
 * is a sibling div carrying the article elements the playhead's jump targets.
 */
function renderOverlay({
  threads,
  messagesByThread,
  activeThreadId = null,
  conversationArticles = [] as { uuid: string }[],
}: {
  threads: Thread[];
  messagesByThread: Map<number, Message[]>;
  activeThreadId?: number | null;
  conversationArticles?: { uuid: string }[];
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
  vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
    async (threadId) => ({
      messages: messagesByThread.get(threadId as number) ?? [],
    }),
  );
  const bodyRef = createRef<HTMLDivElement>();
  const result = render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={apiClient}>
        <div>
          <div ref={bodyRef} data-testid="conversation-body">
            {conversationArticles.map((a) => (
              <article key={a.uuid} data-message-uuid={a.uuid}>
                {a.uuid}
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
    </QueryClientProvider>,
  );
  return { ...result, apiClient, bodyRef };
}

/**
 * Reset the cross-test global state the overlay touches: localStorage (the
 * collapse preference) and the navStore (active thread / focused session).
 */
function resetGlobals() {
  window.localStorage.clear();
  useNavStore.setState({
    focusedSessionId: null,
    activeThreadId: null,
    preNewSessionFocus: null,
    settingsOpen: false,
  });
}

/**
 * Stub the first lane axis row's bounding rect so click-to-jump tests can
 * supply deterministic playhead coordinates without measuring real layout
 * (jsdom does not run CSS, so every rect is 0 by default).
 */
function stubAxisRect(rect: Partial<DOMRect>): void {
  const original = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
    function (this: HTMLElement) {
      if (this.hasAttribute('data-timeline-axis')) {
        return {
          left: 0,
          top: 0,
          right: 240,
          bottom: 18,
          width: 240,
          height: 18,
          x: 0,
          y: 0,
          toJSON: () => ({}),
          ...rect,
        } as DOMRect;
      }
      return original.call(this);
    },
  );
}

describe('ThreadTimelineOverlay collapse toggle', () => {
  beforeEach(() => {
    resetGlobals();
  });

  it('starts collapsed when no preference has been saved', () => {
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    const toggle = screen.getByTestId('thread-timeline-toggle');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('thread-timeline-body')).toBeNull();
  });

  it('toggles open on click and persists the preference', () => {
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('thread-timeline-body')).toBeInTheDocument();
    expect(window.localStorage.getItem(TIMELINE_EXPANDED_STORAGE_KEY)).toBe(
      'true',
    );
  });

  it('restores the persisted expanded preference on mount', () => {
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
  });

  it('toggles closed again and persists the change', () => {
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');
    expect(window.localStorage.getItem(TIMELINE_EXPANDED_STORAGE_KEY)).toBe(
      'false',
    );
  });
});

describe('ThreadTimelineOverlay lane labels', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it("uses the wire thread.title as the lane label, matching Navigator", async () => {
    const subTitle = 'Investigate the staging migration failure end to end';
    const threads = [
      makeThread(1, {
        // The main thread's wire title is typically the session prompt; the
        // lane labels it `"main"` regardless, mirroring Navigator's omission
        // of the main row and the breadcrumb's left-most "main" crumb.
        title: 'a long session prompt the server stored here',
        created_at: '2026-01-01T00:00:00Z',
      }),
      makeThread(2, {
        title: subTitle,
        parent_thread_id: 1,
        root_message_uuid: 'root-of-sub',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const labels = await screen.findAllByTestId('thread-timeline-lane-label');
    expect(labels[0]).toHaveTextContent('main');
    expect(labels[0]).toHaveAttribute('title', 'main');
    // Sub-thread label is the wire title verbatim; CSS `truncate` shortens
    // visually but the full title remains in the DOM and in the tooltip.
    expect(labels[1]).toHaveTextContent(subTitle);
    expect(labels[1]).toHaveAttribute('title', subTitle);
  });

  it('falls back to `thread <id>` when the wire title is empty', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        title: '',
        parent_thread_id: 1,
        root_message_uuid: 'root-uuid-only',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const labels = await screen.findAllByTestId('thread-timeline-lane-label');
    expect(labels[1]).toHaveTextContent('thread 2');
    expect(labels[1]).toHaveAttribute('title', 'thread 2');
  });
});

describe('ThreadTimelineOverlay playhead', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('renders one playhead per lane so the scrub indicator scrolls with the body', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const playheads = await screen.findAllByTestId('thread-timeline-playhead');
    expect(playheads).toHaveLength(2);
  });

  it('does not navigate when a dot is merely hovered (no hover-jump)', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const threads = [makeThread(1)];
    // Use large (user-text) messages so they render as individual dots; two
    // consecutive empty-content user messages would now collapse into a
    // cluster (see small-dot clustering) and there would be no individual
    // `msg-a` dot to hover.
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
    // The initial-settle is intentionally inert: the playhead lands on the
    // latest dot but does NOT scroll the pane (or switch threads) until the
    // user scrubs — so no one ever expects to see scrollIntoView called yet.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    expect(dots).toHaveLength(2);
    expect(scrollIntoView).not.toHaveBeenCalled();
    // Hovering and leaving any dot must not move the playhead or jump.
    const dotA = dots.find((d) => d.getAttribute('data-message-uuid') === 'msg-a');
    fireEvent.mouseEnter(dotA!);
    fireEvent.mouseLeave(dotA!);
    // Give microtasks a chance to run; nothing should fire from a hover.
    await Promise.resolve();
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it('jumps the playhead to a clicked x and scrolls the matching message into view', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Use large (user-text) messages so each renders as an individual dot
    // and a click can directly target msg-a's x without going through a
    // cluster mark.
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
    // Wait for the data-driven layout (dots + playhead) to land. The initial
    // settle is intentionally silent; the click below is the only thing that
    // should ever call scrollIntoView in this test.
    await screen.findAllByTestId('thread-timeline-dot');
    expect(scrollIntoView).not.toHaveBeenCalled();
    // The axis width is stubbed at 240; clicking at x=0 lands the playhead at
    // fraction 0, which is msg-a (the earliest message).
    fireEvent.click(screen.getByTestId('thread-timeline-body'), {
      clientX: 0,
    });
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(1));
    const target = within(screen.getByTestId('conversation-body')).getByText(
      'msg-a',
    );
    expect(scrollIntoView.mock.instances[0]).toBe(target);
  });

  it('switches the active thread and defers the scroll until the next frame when the playhead lands in another lane', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    // Capture rAF callbacks so the test can drive them after re-rendering
    // with the target lane's article in the DOM — mirroring how the live
    // app re-renders the conversation pane on active-thread change.
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
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: 'msg-a',
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      const messages = new Map([
        [
          1,
          [
            makeMessage(1, 0, 'msg-a', {
              created_at: '2026-01-01T00:00:00Z',
            }),
          ],
        ],
        [
          2,
          [
            makeMessage(2, 0, 'msg-b', {
              created_at: '2026-01-01T00:02:00Z',
            }),
          ],
        ],
      ]);
      // Start with lane 1 active; only the active lane's article (msg-a) is
      // rendered, mirroring the live app where the conversation pane only
      // holds the active thread's messages.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [{ uuid: 'msg-a' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // The initial settle does NOT scroll on first mount (the user has
      // not asked to be moved), so the click below is the only thing that
      // should ever drive a thread switch or scroll in this test.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Click at the right edge: msg-b sits at x=1 on lane 2 (cross-lane).
      fireEvent.click(screen.getByTestId('thread-timeline-body'), {
        clientX: 240,
      });
      // The active thread must flip to msg-b's lane (thread 2).
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      // The scroll is deferred to the next frame; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Re-render with the target lane's article in the DOM and the active
      // thread updated, mirroring the live app's response to the switch.
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
                activeThreadId={2}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Drain the captured rAF callbacks now that the DOM holds msg-b.
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
    // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('240px');
    // A single sub-notch wheel-up event (cumulative |delta| under the
    // first staircase threshold) lands in the slowest bucket → exactly
    // one step back. preventDefault is also called so the page scroll
    // does not run alongside the navigation step.
    const body = screen.getByTestId('thread-timeline-body');
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
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('120px');
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
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('240px');
    const body = screen.getByTestId('thread-timeline-body');
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
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('240px');
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
      const body = screen.getByTestId('thread-timeline-body');
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('0px');
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('0px');
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
      const body = screen.getByTestId('thread-timeline-body');
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

  it('advances exactly one large-message step on a single sub-notch turn', async () => {
    // Single wheel event with |deltaY| = 50 — below the first staircase
    // threshold (100), so the accumulator lands in the slowest bucket (1
    // step). Five large turns so any off-by-one in the calculator would
    // surface as a wrong landing px.
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'm0', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'm1', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'm2', '2026-01-01T00:02:00Z'),
            makeUserText(1, 3, 'm3', '2026-01-01T00:03:00Z'),
            makeUserText(1, 4, 'm4', '2026-01-01T00:04:00Z'),
          ],
        ],
      ]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial active = m4 (x=1 → 240px).
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('240px');
      const body = screen.getByTestId('thread-timeline-body');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // m3 sits at x=0.75 → 180px — one step back, sub-notch event.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('180px');
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('accelerates a fast burst across multiple steps via the staircase', async () => {
    // Five wheel events each at one notch (|deltaY| = 100), all within
    // ~50 ms of each other. Cumulative |delta| after the fifth event is
    // 500 px → the 300-bucket gives 3 steps on the fifth event, while
    // earlier events bumped the playhead through smaller buckets. The
    // assertion is on the final landing position, which captures the full
    // burst's net advancement: from m9 (start) past several intermediate
    // steps to m1 (4th from the start, after 1 + 1 + 2 + 2 + 3 = 9 steps
    // backward, clamped at m0). The exact landing is robust against
    // staircase tuning so long as the burst trips at least the 300 bucket.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 10; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      const messages = new Map([[1, msgs]]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-body');
      // Five back-to-back wheel-up events, 50 ms apart (all inside the
      // 250 ms rolling window).
      for (let i = 0; i < 5; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -100,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += 50;
      }
      // Cumulative steps walked backward across the five events (each
      // event reads the cumulative AFTER its own contribution lands, so
      // the first event already hits the 100 bucket → 2 steps):
      //   cum=100 → bucket 100 (2) → m9 → m7
      //   cum=200 → bucket 100 (2) → m7 → m5
      //   cum=300 → bucket 300 (3) → m5 → m2
      //   cum=400 → bucket 300 (3) → m2 → m0 (clamped after 2)
      //   cum=500 → bucket 300 (3) → clamped at m0
      // The clamp at m0 (x=0) is the final landing.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('0px');
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('resets the accumulator after the rolling window elapses', async () => {
    // Two sub-notch events separated by a gap longer than the window. Each
    // event alone contributes 50 px (bucket 0 → 1 step). With reset, the
    // total is 2 steps; without reset, the second event's cumulative would
    // be 100 (bucket 100 → 2 steps) and the total would be 3 — pinning the
    // reset behaviour by total advancement.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-body');
      // First event: cum=50 → 1 step back (m5 → m4).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Wait longer than the window so the accumulator resets.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 150;
      // Second event after the gap: fresh cum=50 → 1 step back (m4 → m3).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages at x = 0, 48, 96, 144, 192, 240. m3 sits at x = 144.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('144px');
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('caps a single trackpad-sized event at the slowest bucket via the per-event clamp', async () => {
    // A single trackpad-sized event (|deltaY| = 10) lands in the slowest
    // staircase bucket (1 step) regardless of `deltaMode`. The clamp's
    // role is preventing a single noisy event from skipping straight to
    // the top bucket; this test pins that contract by dispatching the
    // burst's first event in isolation and asserting it advances exactly
    // one large step (rather than e.g. ten, which would happen if the
    // calculator multiplied steps by event count without going through
    // the staircase).
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-body');
      // First trackpad event of a burst (|deltaY| = 10). Cumulative is 10
      // → bucket 0 → 1 step back. m5 → m4 (x=192 on the 6-msg axis).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -10,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages → 5 gaps → 240 / 5 = 48 px each. m4 sits at x=192.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('192px');
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('treats deltaMode=1 (line) as ~40 px per line via normalization', async () => {
    // A line-mode event with deltaY = 3 must behave like a pixel-mode
    // event of ~120 px — i.e. cross the 100-bucket threshold and walk
    // two large-message steps.
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-body');
      // line-mode event, 3 lines back → 3 * 40 = 120 px → 2-step bucket.
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -3,
            deltaMode: 1,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages at x = 0, 48, 96, 144, 192, 240. Starting on m5 (x=240),
      // two steps back → m3 (x=144).
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe('144px');
    } finally {
      nowSpy.mockRestore();
    }
  });
});

describe('ThreadTimelineOverlay wheel calculator', () => {
  it('normalizes pixel-mode |delta| with the per-event clamp', () => {
    expect(normalizeWheelDeltaPx(50, 0)).toBe(50);
    expect(normalizeWheelDeltaPx(-50, 0)).toBe(50);
    // Above the clamp ceiling, contributions are capped.
    expect(normalizeWheelDeltaPx(500, 0)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('normalizes line-mode |delta| by the per-line pixel proxy', () => {
    // 1 line ≈ 40 px, two lines ≈ 80 px (under the clamp).
    expect(normalizeWheelDeltaPx(2, 1)).toBe(2 * WHEEL_DELTA_LINE_PX);
    // 5 lines = 200 px → clamped to 100.
    expect(normalizeWheelDeltaPx(5, 1)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('normalizes page-mode |delta| by the per-page pixel proxy and clamps', () => {
    // Even a single page-mode event is clamped to one notch.
    expect(normalizeWheelDeltaPx(1, 2)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('maps cumulative |delta| to the staircase step count', () => {
    expect(stepsForCumulativePx(0)).toBe(1);
    expect(stepsForCumulativePx(99)).toBe(1);
    expect(stepsForCumulativePx(100)).toBe(2);
    expect(stepsForCumulativePx(299)).toBe(2);
    expect(stepsForCumulativePx(300)).toBe(3);
    expect(stepsForCumulativePx(599)).toBe(3);
    expect(stepsForCumulativePx(600)).toBe(5);
    expect(stepsForCumulativePx(999)).toBe(5);
    expect(stepsForCumulativePx(1000)).toBe(8);
    expect(stepsForCumulativePx(10_000)).toBe(8);
  });
});

describe('ThreadTimelineOverlay mark rendering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('renders circular marks with role-coded color classes and a data-message-kind attribute', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // A genuine human turn (user role + text block) → `user` kind.
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          // An assistant reply → `other` kind.
          makeMessage(1, 1, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    expect(marks).toHaveLength(2);
    const userMark = marks.find(
      (m) => m.getAttribute('data-message-uuid') === 'u',
    )!;
    const otherMark = marks.find(
      (m) => m.getAttribute('data-message-uuid') === 'a',
    )!;
    // Circle: rounded-full with equal width/height. The packed-lane overlap
    // problem that drove v3's rectangles is solved at the layout level now
    // (see buildGlobalXMap's minimum-spacing push), so the marks can stay
    // solid-fill with no alpha/ring workaround.
    expect(userMark.className).toContain('rounded-full');
    expect(userMark.style.width).toBe(userMark.style.height);
    // Role-coded color and data attribute (tested via class membership and
    // the data attribute, not literal hex, so the tailwind tokens can move).
    // Solid fill on both — overlap is prevented by the global x map, not by
    // alpha stacking, so the classes carry no alpha suffix or ring outline.
    expect(userMark).toHaveAttribute('data-message-kind', 'user');
    expect(userMark.className).toContain('bg-blue-500');
    expect(userMark.className).not.toContain('bg-blue-500/');
    expect(userMark.className).not.toContain('ring-');
    expect(otherMark).toHaveAttribute('data-message-kind', 'other');
    expect(otherMark.className).toContain('bg-slate-400');
    expect(otherMark.className).not.toContain('bg-slate-400/');
    expect(otherMark.className).not.toContain('ring-');
  });

  it('renders the main-conversation turns as a larger circle than the auxiliary turns', async () => {
    // One small dot sandwiched between two large dots on either side so
    // each small dot stays a lone single-dot render item (the clustering
    // logic needs 2+ adjacent smalls — see the small-dot clustering tests).
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // user turn → large
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          // tool call → small (sandwiched between u and a, no cluster)
          makeMessage(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          // assistant prose → large
          makeMessage(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:02:00Z',
          }),
          // meta line → small (lone, between large a and large u2)
          makeMessage(1, 3, 'm', {
            role: 'meta',
            content: [{ type: 'text', text: 'sys' }],
            created_at: '2026-01-01T00:03:00Z',
          }),
          // user turn → large (caps the trailing lone small)
          makeMessage(1, 4, 'u2', {
            role: 'user',
            content: [{ type: 'text', text: 'bye' }],
            created_at: '2026-01-01T00:04:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    const byUuid = new Map(
      marks.map((m) => [m.getAttribute('data-message-uuid'), m]),
    );
    expect(byUuid.get('u')).toHaveAttribute('data-message-size', 'large');
    expect(byUuid.get('a')).toHaveAttribute('data-message-size', 'large');
    expect(byUuid.get('t')).toHaveAttribute('data-message-size', 'small');
    expect(byUuid.get('m')).toHaveAttribute('data-message-size', 'small');
    // The diameter of a "large" mark is greater than that of a "small" one
    // (px values, not classes — the renderer applies them inline).
    const largeDiameter = parseFloat(byUuid.get('u')!.style.width);
    const smallDiameter = parseFloat(byUuid.get('t')!.style.width);
    expect(largeDiameter).toBeGreaterThan(smallDiameter);
    // The delta is subtle but visible — the lane should still read as one
    // timeline, not two layers. Cap at 6 px so a future tweak that goes
    // overboard is caught here.
    expect(largeDiameter - smallDiameter).toBeLessThanOrEqual(6);
  });
});

describe('ThreadTimelineOverlay active lane highlight', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('falls back to the activeThreadId prop highlight when no dot is in view', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: 'uuid-a',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map(), activeThreadId: 2 });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
  });

  it('marks the lane containing the playhead-active message regardless of the activeThreadId prop', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    const messages = new Map([
      [1, [makeMessage(1, 0, 'a', { created_at: '2026-01-01T00:00:00Z' })]],
      [2, [makeMessage(2, 0, 'b', { created_at: '2026-01-01T00:02:00Z' })]],
    ]);
    // The playhead's initial position is the latest dot (msg-b on lane 2),
    // so the lane-2 highlight follows the playhead even when prop says lane 1.
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'a' }, { uuid: 'b' }],
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    await waitFor(() => {
      expect(lanes[1]).toHaveAttribute('data-active', 'true');
    });
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
  });
});

describe('ThreadTimelineOverlay wheel skips small marks', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('walks only the main-conversation (large) subset on wheel, jumping over tool calls', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Three large turns interleaved with a small tool call. The wheel-step
    // navigation must skip the tool call so one notch advances by one
    // headline turn — `large-b` should land between `large-a` and
    // `large-c` regardless of the small dot's x.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
          makeUserText(1, 3, 'large-c', '2026-01-01T00:03:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
        { uuid: 'large-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead lands on the latest message (large-c, x=1 → 240px).
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('240px');
    // Wheel up (sub-notch event → one step): the previous LARGE turn is
    // large-b (x=2/3 → 160px), NOT the small tool call between them. The
    // sub-notch keeps the staircase at one step so the assertion targets
    // the immediate large neighbour.
    const body = screen.getByTestId('thread-timeline-body');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -50,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('160px');
  });

  it('still allows a click to target a small mark precisely', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // A small tool call between two large turns. A click at the tool call's
    // x must jump the playhead to it, even though the wheel would skip it.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            // Land at x=0.5 (the midpoint) so the click target is
            // unambiguous and deterministic.
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Click at x=120 (midpoint) → small-t is the nearest mark.
    fireEvent.click(screen.getByTestId('thread-timeline-body'), {
      clientX: 120,
    });
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe('120px');
  });
});

describe('ThreadTimelineOverlay jump-target highlight', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('highlights the destination message after a click jump so the eye spots it', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
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
    // Click on x=0 (msg-a) to drive a same-lane jump.
    fireEvent.click(screen.getByTestId('thread-timeline-body'), {
      clientX: 0,
    });
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    // The destination article carries the highlight class right after the
    // scroll lands. We assert the class is present rather than waiting for
    // its removal — the CSS animation fades the bubble background back to
    // its rest color, the class is the trigger.
    const target = within(screen.getByTestId('conversation-body')).getByText(
      'msg-a',
    );
    expect(target.classList.contains(TIMELINE_JUMP_HIGHLIGHT_CLASS)).toBe(true);
  });
});

describe('ThreadTimelineOverlay does not override an external active-thread change', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
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
    const body = screen.getByTestId('thread-timeline-body');
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

describe('ThreadTimelineOverlay mystery-dot filter', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('does not render dots for system or other ingest-only rows', async () => {
    // Real sessions emit a handful of `role: "system"` rows on startup
    // (and an occasional `other`) whose stamps land before the first
    // user prompt. The transcript skips them; the timeline must too so
    // they do not surface as mystery dots to the left of the first
    // human-readable message.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'sys', {
            role: 'system',
            content: [{ type: 'text', text: 'bootstrap' }],
            created_at: '2025-12-31T23:59:00Z',
          }),
          makeMessage(1, 1, 'usr', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 2, 'oth', {
            role: 'other',
            content: [{ type: 'text', text: 'misc' }],
            created_at: '2026-01-01T00:00:30Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    const uuids = marks
      .map((m) => m.getAttribute('data-message-uuid'))
      .sort();
    expect(uuids).toEqual(['usr']);
  });
});

describe('ThreadTimelineOverlay small-dot clustering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('renders 2+ consecutive small dots as a single cluster mark', async () => {
    // A user turn, three tool calls in a row (each is a "small" auxiliary
    // mark), then an assistant prose reply. The three tool calls must
    // collapse into one cluster mark while the user and assistant turns
    // still render as their own dots.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'do stuff' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
          makeMessage(1, 2, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:20Z',
          }),
          makeMessage(1, 3, 't3', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu3', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:30Z',
          }),
          makeMessage(1, 4, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    // The lone large dots (u, a) still render as dots.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    const dotUuids = dots.map((d) => d.getAttribute('data-message-uuid'));
    expect(dotUuids).toContain('u');
    expect(dotUuids).toContain('a');
    expect(dotUuids).not.toContain('t1');
    expect(dotUuids).not.toContain('t2');
    expect(dotUuids).not.toContain('t3');
    // Exactly one cluster mark, pointing at the first member.
    const clusters = await screen.findAllByTestId(
      'thread-timeline-cluster',
    );
    expect(clusters).toHaveLength(1);
    expect(clusters[0]).toHaveAttribute('data-message-uuid', 't1');
    expect(clusters[0]).toHaveAttribute('data-cluster-member-count', '3');
  });

  it('renders a lone small dot as a regular dot, not a cluster', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:30Z',
          }),
          makeMessage(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    const dotUuids = dots.map((d) => d.getAttribute('data-message-uuid'));
    expect(dotUuids).toContain('t');
    // No cluster mark when only one consecutive small dot exists.
    expect(screen.queryAllByTestId('thread-timeline-cluster')).toHaveLength(
      0,
    );
  });
});

describe('ThreadTimelineOverlay sticky lane labels', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('pins the label column to the body left during horizontal scroll', async () => {
    // The label spans the LABEL_COLUMN_PX strip and must stay visible
    // when the body scrolls horizontally past a long axis. CSS `position:
    // sticky; left: 0` is the contract; we assert the inline class names
    // (jsdom does not compute layout, so position/left are tested at the
    // style/class level — the real browser provides the actual sticky
    // behaviour).
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const label = await screen.findByTestId('thread-timeline-lane-label');
    expect(label.className).toMatch(/\bsticky\b/);
    expect(label.className).toMatch(/\bleft-0\b/);
    // Label needs an opaque background so axis dots cannot show through
    // it once the body scrolls horizontally — match `bg-white` (inactive)
    // or `bg-slate-50` (active highlight).
    expect(label.className).toMatch(/bg-(white|slate-50)/);
  });

  it('sizes the lane list to the full axis width so sticky spans the whole scroll range', async () => {
    // Regression: v7 sized the lane `<ul>` only to the body's content
    // width (default column-flex), so each `<li>` was body-wide while its
    // `shrink-0` children (label + axis) overflowed to the right. The
    // sticky label's containing block — the `<li>` — then ended at the
    // body's right edge; once the user scrolled past
    // `(li_width - LABEL_COLUMN_PX)` the label hit the `<li>`'s right
    // edge and stopped following the viewport, sliding leftward out of
    // view. The fix is `w-max min-w-full` on the `<ul>` so the list
    // grows to the natural content width and sticky spans the FULL
    // scroll range.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    // The lane list is the only `<ul>` inside the body.
    const body = await screen.findByTestId('thread-timeline-body');
    const ul = body.querySelector('ul');
    expect(ul).not.toBeNull();
    // `w-max` widens the list to its widest lane, `min-w-full` keeps it
    // at least body-wide so a short session does not collapse.
    expect(ul!.className).toMatch(/\bw-max\b/);
    expect(ul!.className).toMatch(/\bmin-w-full\b/);
  });

  it('keeps the sticky label rendered when the body is scrolled to its maximum', async () => {
    // Drive the body's `scrollLeft` to the full scroll range and assert
    // the sticky label is still in the DOM with its `position: sticky;
    // left: 0` contract intact. jsdom does not compute layout, so we
    // cannot read a bounding rect that reflects sticky; the structural
    // checks (class still applied, element still mounted at the
    // leftmost position of its lane) are the testable surface for the
    // FULL-scroll-range guarantee.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const body = await screen.findByTestId('thread-timeline-body');
    // Stub the scroll geometry: a body of 200px viewport with 1200px of
    // content (so a real browser scrolls 1000px horizontally before the
    // right edge); set scrollLeft to that maximum.
    Object.defineProperty(body, 'clientWidth', { value: 200, configurable: true });
    Object.defineProperty(body, 'scrollWidth', { value: 1200, configurable: true });
    body.scrollLeft = body.scrollWidth - body.clientWidth;
    // The label is still the first child of its `<li>` (sticky preserves
    // DOM order — only paint shifts), and the sticky/left-0 contract is
    // still on it. A regression where the label gets unmounted, hidden,
    // or loses the sticky class would fail here.
    const lane = body.querySelector('li[data-testid="thread-timeline-lane"]');
    expect(lane).not.toBeNull();
    const label = lane!.querySelector('[data-testid="thread-timeline-lane-label"]');
    expect(label).not.toBeNull();
    expect(label).toBe(lane!.firstElementChild);
    expect((label as HTMLElement).className).toMatch(/\bsticky\b/);
    expect((label as HTMLElement).className).toMatch(/\bleft-0\b/);
  });
});
