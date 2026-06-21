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
  ALL_ARTICLES_SELECTOR,
  articleMessageSelector,
  LANE_LEFT_PAD_PX,
  MARK_CLUSTER_PX,
  MARK_CLUSTER_RING_COLOR,
  MARK_SMALL_PX,
  PANE_SCROLL_DEBOUNCE_MS,
  PANE_SCROLL_OBSERVER_THRESHOLD,
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
  SCROLL_DOM_READY_TIMEOUT_MS,
  ThreadTimelineOverlay,
  TIMELINE_EXPANDED_STORAGE_KEY,
  TIMELINE_JUMP_HIGHLIGHT_CLASS,
  WHEEL_DELTA_LINE_PX,
  WHEEL_PER_EVENT_CLAMP_PX,
  WHEEL_VELOCITY_WINDOW_MS,
  normalizeWheelDeltaPx,
  scheduleScrollAfterRender,
  scrollMessageIntoView,
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
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
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
      fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
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
      // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
    // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
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
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
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
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
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
      // m3 sits at x=0.75 → 180px — one step back, sub-notch event.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${180 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('accelerates a fast burst across multiple steps via the staircase', async () => {
    // Five wheel events each at one notch (|deltaY| = 100), all within
    // ~50 ms of each other. The first event sits in the slowest bucket
    // (1 step — a single notch never accelerates) and later events trip
    // the higher buckets as their cumulative |delta| grows inside the
    // rolling window. The assertion is on the final landing position,
    // which captures the full burst's net advancement.
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
      const body = screen.getByTestId('thread-timeline-axis-column');
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
      // event reads the cumulative AFTER its own contribution lands).
      // The first notch always sits in the slowest bucket (1 step) so
      // the user can always land on the immediate neighbour:
      //   cum=100 → bucket 0   (1) → m9 → m8
      //   cum=200 → bucket 200 (2) → m8 → m6
      //   cum=300 → bucket 200 (2) → m6 → m4
      //   cum=400 → bucket 400 (3) → m4 → m1
      //   cum=500 → bucket 400 (3) → m1 → m0 (clamped after 1)
      // The clamp at m0 (x=0) is the final landing.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
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
      const body = screen.getByTestId('thread-timeline-axis-column');
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
      ).toBe(`${144 + LANE_LEFT_PAD_PX}px`);
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
      const body = screen.getByTestId('thread-timeline-axis-column');
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
      ).toBe(`${192 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('treats deltaMode=1 (line) as ~40 px per line via normalization', async () => {
    // A line-mode event with |deltaY| = 3 must behave like a pixel-mode
    // event of ~120 px — i.e. clamped at the per-event cap (100 px) and
    // tallied into the rolling-window accumulator at one notch's worth.
    // Two such back-to-back events together push the cumulative above
    // the 2-step bucket (200 px) so the burst walks 1 then 2 steps —
    // 3 large-message steps in total, mirroring how a real line-mode
    // device emits per-tick scrolls.
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
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Two line-mode events 50 ms apart, each 3 lines back → each
      // contributes 3 * 40 = 120 px clamped to 100 → cum=100 then 200.
      // Walks 1 step then 2 steps: m5 → m4 → m2.
      for (let i = 0; i < 2; i += 1) {
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
        nowMs += 50;
      }
      // Six messages at x = 0, 48, 96, 144, 192, 240. Starting on m5
      // (x=240), three steps back → m2 (x=96).
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${96 + LANE_LEFT_PAD_PX}px`);
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
    // The first acceleration bucket sits strictly above one notch's worth
    // of clamped |delta| (WHEEL_PER_EVENT_CLAMP_PX = 100), so a single
    // slow notch (cum=100, the maximum after one event) still walks just
    // one step — the user can land on the immediate prev/next message.
    expect(stepsForCumulativePx(100)).toBe(1);
    expect(stepsForCumulativePx(199)).toBe(1);
    expect(stepsForCumulativePx(200)).toBe(2);
    expect(stepsForCumulativePx(399)).toBe(2);
    expect(stepsForCumulativePx(400)).toBe(3);
    expect(stepsForCumulativePx(699)).toBe(3);
    expect(stepsForCumulativePx(700)).toBe(5);
    expect(stepsForCumulativePx(1099)).toBe(5);
    expect(stepsForCumulativePx(1100)).toBe(8);
    expect(stepsForCumulativePx(10_000)).toBe(8);
  });

  it('guarantees at least one step for any nonzero accumulator value', () => {
    // Regression: a single slow wheel notch (any |delta| up to the
    // per-event clamp of 100 px) must always advance exactly one step so
    // the user can always land on the immediate prev/next message. The
    // first acceleration bucket sits strictly above that ceiling.
    expect(stepsForCumulativePx(1)).toBe(1);
    expect(stepsForCumulativePx(WHEEL_PER_EVENT_CLAMP_PX)).toBe(1);
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
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // Wheel up (sub-notch event → one step): the previous LARGE turn is
    // large-b (x=2/3 → 160px), NOT the small tool call between them. The
    // sub-notch keeps the staircase at one step so the assertion targets
    // the immediate large neighbour.
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
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${160 + LANE_LEFT_PAD_PX}px`);
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
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
      clientX: 120,
    });
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
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
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
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

describe('ThreadTimelineOverlay two-column layout', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('renders the label column and axis column as siblings inside the body', async () => {
    // Structural contract: the timeline body is a flex row holding two
    // sibling columns — a static label column on the left, and the
    // horizontally-scrollable axis column on the right. Dots and the
    // playhead live inside the axis column so they can pan out from
    // under the labels without sliding behind them (the v9 sticky-label
    // layout had dots passing behind the labels during horizontal pan).
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const labelColumn = await screen.findByTestId(
      'thread-timeline-label-column',
    );
    const axisColumn = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    // Both columns are direct children of the body — siblings, not
    // nested — so the body's flex row lays them out side by side.
    const body = screen.getByTestId('thread-timeline-body');
    expect(labelColumn.parentElement).toBe(body);
    expect(axisColumn.parentElement).toBe(body);
  });

  it('isolates horizontal scroll to the axis column so labels stay put', async () => {
    // The outer body owns the vertical scroll only; the axis column owns
    // horizontal scroll on its own so a wide axis pans under the labels
    // without dragging them along. A regression that reattaches
    // `overflow-x: auto` to the body (or removes it from the axis column)
    // would surface here.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const body = await screen.findByTestId('thread-timeline-body');
    expect(body.className).toMatch(/\boverflow-y-auto\b/);
    expect(body.className).not.toMatch(/\boverflow-x\b/);
    const labelColumn = await screen.findByTestId(
      'thread-timeline-label-column',
    );
    expect(labelColumn.className).not.toMatch(/\boverflow-x\b/);
    const axisColumn = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    expect(axisColumn.className).toMatch(/\boverflow-x-auto\b/);
  });

  it('reflects the lane active highlight in both columns simultaneously', async () => {
    // The active lane is a single visual band that must span both columns
    // — a half-highlighted lane would read as a bug. Each column's
    // matching row carries the same highlight classes (border-y +
    // bg-slate-50) so the two halves line up into one continuous band.
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const labelRows = await screen.findAllByTestId(
      'thread-timeline-lane-label-row',
    );
    const axisRows = await screen.findAllByTestId('thread-timeline-lane');
    // Both lanes are present in both columns and indexed in the same
    // order — so the active highlight on lane 2 lands at the same row
    // index on both sides.
    expect(labelRows).toHaveLength(2);
    expect(axisRows).toHaveLength(2);
    expect(labelRows[1]).toHaveAttribute('data-active', 'true');
    expect(axisRows[1]).toHaveAttribute('data-active', 'true');
    expect(labelRows[0]).toHaveAttribute('data-active', 'false');
    expect(axisRows[0]).toHaveAttribute('data-active', 'false');
    // The visual highlight tokens are identical so the band reads as
    // continuous across the two columns.
    expect(labelRows[1].className).toMatch(/bg-slate-50/);
    expect(axisRows[1].className).toMatch(/bg-slate-50/);
    expect(labelRows[1].className).toMatch(/border-slate-200/);
    expect(axisRows[1].className).toMatch(/border-slate-200/);
  });

  it('keeps the label column out of the wheel-scrub scope', async () => {
    // A wheel event over the label column must NOT scrub the timeline —
    // labels behave like normal page content. The wheel listener attaches
    // to the axis column alone, and wheel events do not bubble from a
    // parent to a child (axis is the body's sibling of labels, not its
    // descendant), so dispatching the wheel on the label column should
    // leave the playhead untouched.
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
    // Initial playhead lands on the latest message (msg-b, x=1 → 240px).
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // Wheel-up on the label column has no effect.
    const labelColumn = screen.getByTestId('thread-timeline-label-column');
    act(() => {
      labelColumn.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // A wheel on the axis column DOES scrub, proving the listener is
    // wired — just scoped to the right column. With only two messages
    // and the playhead starting on the last one, one step back lands on
    // msg-a at x=0.
    const axisColumn = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      axisColumn.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${LANE_LEFT_PAD_PX}px`);
  });

  it('ignores clicks on the label column', async () => {
    // Same scope contract for clicks: a click on a label is not a scrub
    // intent. The handler attaches to the axis column only.
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
    // The playhead initially sits on msg-b (x=240).
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // A click on the label column with clientX=0 (where msg-a would land
    // if the axis click handler picked it up) must NOT move the playhead.
    fireEvent.click(screen.getByTestId('thread-timeline-label-column'), {
      clientX: 0,
    });
    expect(
      screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });
});

describe('ThreadTimelineOverlay cluster mark size (v11 Improvement 1)', () => {
  // v10 dogfooding revealed that the cluster's render size (5 px) was
  // visually indistinguishable from the 6 px main-role dots — the user
  // could not tell a run-of-tool-calls cluster apart from a user/Claude
  // turn. The contract now is: a cluster renders at the SMALL dot
  // diameter exactly, and conveys "cluster-ness" through a thin outline
  // ring instead of size. These tests pin the contract.

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('renders cluster dots at exactly the small-dot diameter', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'go' }],
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
          makeMessage(1, 3, 'a', {
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
    const clusters = await screen.findAllByTestId('thread-timeline-cluster');
    expect(clusters).toHaveLength(1);
    const cluster = clusters[0];
    expect(cluster.style.width).toBe(`${MARK_SMALL_PX}px`);
    expect(cluster.style.height).toBe(`${MARK_SMALL_PX}px`);
    // Pin the literal value too — the v7/v11 contract is "cluster dots
    // stay at 4px exactly, ring-only differentiation". A future tweak to
    // MARK_SMALL_PX should break this assertion so the regression is
    // visible at review time, not in dogfooding.
    expect(cluster.style.width).toBe('4px');
    expect(cluster.style.height).toBe('4px');
    // Cross-check the constant equality so a future "let's bump cluster
    // size again" lands here, not in dogfooding.
    expect(MARK_CLUSTER_PX).toBe(MARK_SMALL_PX);
    expect(MARK_CLUSTER_PX).toBe(4);
  });

  it('paints the cluster ring outline so a cluster reads as a small dot with a halo', async () => {
    // The cluster's distinguishing feature is the ring, NOT its size. A
    // regression that drops the outline (or paints the same outline on
    // every dot) is what these assertions guard against. Inner fill is
    // the same slate-400 a small assistant dot uses, so the ring is
    // strictly additive.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const cluster = (await screen.findAllByTestId('thread-timeline-cluster'))[0];
    expect(cluster.className).toMatch(/\boutline\b/);
    expect(cluster.className).toMatch(/\boutline-1\b/);
    expect(cluster.className).toMatch(
      new RegExp(`\\b${MARK_CLUSTER_RING_COLOR}\\b`),
    );
  });
});

describe('ThreadTimelineOverlay scheduleScrollAfterRender DOM-ready wait (v11 Improvement 2)', () => {
  // v10's cross-lane jump deferred the scroll a single rAF; when the
  // subthread switch re-render took 2+ frames (which it usually does),
  // querySelector found no target and the scroll silently dropped. The
  // new behaviour polls each rAF until the uuid is in the DOM, capped
  // by SCROLL_DOM_READY_TIMEOUT_MS.

  beforeEach(() => {
    resetGlobals();
  });

  it('waits across multiple rAFs for the target element, then scrolls when it appears', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    // Capture rAF callbacks so we can drive them one frame at a time.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* cancellation is exercised elsewhere */
    }) as typeof window.cancelAnimationFrame;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        const cancel = scheduleScrollAfterRender(container, 'late-uuid');
        // First two ticks: target absent, scroll must NOT fire.
        expect(rafCallbacks).toHaveLength(1);
        let cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).not.toHaveBeenCalled();
        // Polling re-queues itself for the next frame.
        expect(rafCallbacks).toHaveLength(1);
        cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(1);
        // Third tick: target is now in the DOM (mirrors a real cross-lane
        // re-render that took 3 frames). The scroll fires this tick.
        const target = document.createElement('article');
        target.setAttribute('data-message-uuid', 'late-uuid');
        container.appendChild(target);
        cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(1);
        expect(scrollIntoView.mock.instances[0]).toBe(target);
        cancel();
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('gives up after SCROLL_DOM_READY_TIMEOUT_MS when the target never appears', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    const originalPerfNow = window.performance.now;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so the loop crosses the timeout deterministically
    // — first tick at t=0, second at t=TIMEOUT+1 ms.
    let nowValue = 1_000;
    window.performance.now = (() => nowValue) as typeof performance.now;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        // No matching child is ever appended.
        scheduleScrollAfterRender(container, 'never-arrives');
        expect(rafCallbacks).toHaveLength(1);
        let cb = rafCallbacks.shift()!;
        nowValue = 1_000; // first tick: t=0 elapsed
        cb(nowValue);
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(1);
        // Advance past the timeout and tick again: the loop bails without
        // re-queuing and without scrolling.
        nowValue = 1_000 + SCROLL_DOM_READY_TIMEOUT_MS + 1;
        cb = rafCallbacks.shift()!;
        cb(nowValue);
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(0);
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });
});

describe('ThreadTimelineOverlay pane scroll → playhead follow (v11 Improvement 3)', () => {
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

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
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
      const io = fake.instances[fake.instances.length - 1];
      expect(io.options?.threshold).toBe(PANE_SCROLL_OBSERVER_THRESHOLD);
      // Every article in the conversation body is observed by the live
      // observer.
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      for (const a of articles) {
        expect(io.observed.has(a)).toBe(true);
      }
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
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      const io = fake.instances[fake.instances.length - 1];
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
      // Click at x=0: timeline jumps to msg-a (programmatic scroll fires).
      act(() => {
        fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
          clientX: 0,
        });
      });
      // Playhead now at msg-a (x=0).
      await waitFor(() => {
        expect(
          screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
        ).toBe(`${LANE_LEFT_PAD_PX}px`);
      });
      // While inside the guard window, emit an IO entry claiming
      // msg-b is topmost-visible — exactly what the jump's own scroll
      // would produce as it animates past msg-b. The playhead must NOT
      // jump to msg-b.
      const io = fake.instances[fake.instances.length - 1];
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
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
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
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
      // Initial playhead is on msg-b (the latest message, x=240).
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);

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
      const io = fake.instances[fake.instances.length - 1];
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // The playhead is now on msg-a (the oldest message, x=0).
      await waitFor(() => {
        expect(
          screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
        ).toBe(`${LANE_LEFT_PAD_PX}px`);
      });

      // After the superseding jump, the in-flight flag must be cleared so
      // subsequent IO emits are honoured. Emit msg-b (index 1, x=120px) as
      // topmost-visible. The same-lane scroll from the superseding jump also
      // set the time-based guard; advance performance.now past that window
      // too so only the in-flight flag would block the flush (confirming it
      // is cleared by the cancel-with-flag-clear wrapper).
      const originalPerfNow = window.performance.now;
      let nowMs = 20_000;
      window.performance.now = (() => nowMs) as typeof performance.now;
      const io = fake.instances[fake.instances.length - 1];
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
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
        window.performance.now = originalPerfNow;
      }
      // The emit is honoured — the in-flight counter was decremented by the
      // cancel handle when the superseding jump fired, and the time-based
      // guard has also expired. msg-b sits at index 1 → x=120px on the
      // shared axis.
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
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

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
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
      expect(
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);

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
      const io = fake.instances[fake.instances.length - 1];
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('keeps the in-flight counter balanced across a cross-lane chain so a later jump still guards correctly (released-once, no double-decrement)', async () => {
    // The counter is decremented by TWO code paths: scheduleScrollAfterRender's
    // onScroll callback, and the cancel handle (cancelWithCountClear, invoked
    // by a superseding jump or by the cleanup effect on unmount). Both paths
    // can fire for the same jump — every wheel-step beyond the first invokes
    // the previous jump's cancel handle EVEN IF that jump's onScroll has
    // already landed. The `released` flag in the navigation effect ensures
    // each jump decrements the counter at most once.
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
    // prior jump's cancel handle after its onScroll has already fired. If
    // the released-guard were missing, the third jump's counter would not
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
      // navigation effect invokes jump 1's cancel handle FIRST. Jump 1's
      // onScroll has already fired, so a missing released-guard would
      // attempt a second decrement on jump 1 — the clamp prevents wrap,
      // but the accounting is now off-by-one.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
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
      // navigation effect invokes jump 2's cancel handle (jump 2's onScroll
      // already fired). With released-once, the counter should now be 1
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
      // block the IO. With released-once preserving accounting, the counter
      // is at 1; flush bails. Without it the counter is at 0 and the flush
      // would commit msg-b's IO entry (a no-op visually since the playhead
      // is already at msg-b, so we choose msg-a as the IO target — which
      // would snap the playhead BACK to msg-a if the guard were broken).
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
      const io = fake.instances[fake.instances.length - 1];
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
      // the observation to register.
      await Promise.resolve();
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
        screen.getAllByTestId('thread-timeline-playhead')[0].style.left,
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
      window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
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
