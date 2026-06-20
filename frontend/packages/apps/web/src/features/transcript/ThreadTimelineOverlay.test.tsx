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
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'msg-a', { created_at: '2026-01-01T00:00:00Z' }),
          makeMessage(1, 1, 'msg-b', { created_at: '2026-01-01T00:01:00Z' }),
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
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'msg-a', { created_at: '2026-01-01T00:00:00Z' }),
          makeMessage(1, 1, 'msg-b', { created_at: '2026-01-01T00:01:00Z' }),
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
      expect(scrollIntoView).toHaveBeenCalledWith({ block: 'center' });
      const target = within(screen.getByTestId('conversation-body')).getByText(
        'msg-b',
      );
      expect(scrollIntoView.mock.instances[0]).toBe(target);
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('moves the playhead and suppresses page scroll on a wheel event', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Use widely-spread dots so a small wheel shift does not land inside the
    // snap radius, isolating the smooth-scrub math from the snap behaviour
    // (the snap is covered by its own test below).
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'msg-a', { created_at: '2026-01-01T00:00:00Z' }),
          makeMessage(1, 1, 'msg-b', { created_at: '2026-01-01T00:01:00Z' }),
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
    const playheadsBefore = screen.getAllByTestId('thread-timeline-playhead');
    // The initial playhead position is the latest dot's x (1.0). Inspect the
    // first lane's playhead `left` style to confirm the baseline.
    expect(playheadsBefore[0].style.left).toBe('240px');

    // A large negative wheel delta scrolls the playhead toward the left edge,
    // far enough that the snap radius around msg-b (x=1, threshold 0.012) is
    // cleanly escaped so the smooth-scrub displacement is observable.
    const body = screen.getByTestId('thread-timeline-body');
    const wheelEvent = new WheelEvent('wheel', {
      deltaY: -1000,
      bubbles: true,
      cancelable: true,
    });
    const preventDefault = vi.spyOn(wheelEvent, 'preventDefault');
    act(() => {
      body.dispatchEvent(wheelEvent);
    });
    expect(preventDefault).toHaveBeenCalled();
    const playheadsAfter = screen.getAllByTestId('thread-timeline-playhead');
    // -1000 * 0.03 = -30px on a 240px axis = -0.125 fraction shift; from 1.0
    // to 0.875 → 210px. Outside msg-b's snap radius (1 - 0.875 = 0.125 > 0.012)
    // and outside msg-a's snap radius (0.875 > 0.012), so smooth math wins.
    expect(playheadsAfter[0].style.left).toBe('210px');
  });

  it('snaps the playhead onto the nearest mark when a small wheel scrub lands inside the snap radius', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'msg-a', { created_at: '2026-01-01T00:00:00Z' }),
          makeMessage(1, 1, 'msg-b', { created_at: '2026-01-01T00:01:00Z' }),
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
    // A tiny scrub from x=1: -50 * 0.03 = -1.5px on 240px = -0.00625 fraction
    // → 0.99375. That is within 0.012 of msg-b (x=1), so the snap pulls the
    // playhead back onto msg-b's exact x → 240px.
    const body = screen.getByTestId('thread-timeline-body');
    const wheelEvent = new WheelEvent('wheel', {
      deltaY: -50,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      body.dispatchEvent(wheelEvent);
    });
    const playheadsAfter = screen.getAllByTestId('thread-timeline-playhead');
    expect(playheadsAfter[0].style.left).toBe('240px');
  });
});

describe('ThreadTimelineOverlay mark rendering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('renders rectangular marks (not circles) with role-coded color classes and a data-message-kind attribute', async () => {
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
    // Rectangle, not circle: the marker is a `rounded-sm` block with separate
    // width/height (the v2 round dot used `rounded-full` with equal w/h).
    expect(userMark.className).toContain('rounded-sm');
    expect(userMark.className).not.toContain('rounded-full');
    expect(userMark.style.width).not.toBe(userMark.style.height);
    // Role-coded color and data attribute (tested via class membership and
    // the data attribute, not literal hex, so the tailwind tokens can move).
    expect(userMark).toHaveAttribute('data-message-kind', 'user');
    expect(userMark.className).toContain('bg-blue-500');
    expect(otherMark).toHaveAttribute('data-message-kind', 'other');
    expect(otherMark.className).toContain('bg-slate-400');
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
