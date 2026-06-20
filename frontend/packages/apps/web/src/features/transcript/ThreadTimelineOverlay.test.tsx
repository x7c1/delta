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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { Message, Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import {
  ThreadTimelineOverlay,
  TIMELINE_EXPANDED_STORAGE_KEY,
  HOVER_JUMP_DEBOUNCE_MS,
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
 * is a sibling div carrying the article elements the hover-jump targets.
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

  it("uses the root message's body prefix as the lane label, with the full body as the title", async () => {
    const rootUuid = 'root-of-sub';
    const rootBody = 'Investigate the staging migration failure end to end';
    const threads = [
      makeThread(1, {
        created_at: '2026-01-01T00:00:00Z',
      }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: rootUuid,
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    // The subthread's root message lives in the main thread's history.
    const messagesByThread = new Map([
      [
        1,
        [
          makeMessage(1, 0, rootUuid, {
            content_text: rootBody,
            created_at: '2026-01-01T00:00:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({ threads, messagesByThread });
    // The root message lookup needs the per-thread fetch to land before the
    // label resolves from "(no preview)" to the body prefix, so wait for the
    // title attribute to flip rather than reading on the first paint.
    await waitFor(() => {
      const labels = screen.getAllByTestId('thread-timeline-lane-label');
      expect(labels[1]).toHaveAttribute('title', rootBody);
    });
    const labels = screen.getAllByTestId('thread-timeline-lane-label');
    expect(labels[0]).toHaveTextContent('main');
    expect(labels[0]).toHaveAttribute('title', 'main');
    // The trimmed prefix lands in the DOM verbatim; jest-dom's text-content
    // matcher collapses surrounding whitespace, so use the trimmed slice to
    // compare apples to apples without relying on the normaliser's quirks.
    expect(labels[1]).toHaveTextContent(rootBody.slice(0, 24).trim());
  });

  it('falls back to "(no preview)" with the root uuid as the title when the root message body has not loaded yet', async () => {
    const rootUuid = 'root-uuid-only';
    const threads = [
      makeThread(1),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: rootUuid,
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    // Empty messages map: the per-thread fetch resolved to no messages.
    renderOverlay({ threads, messagesByThread: new Map() });
    const labels = await screen.findAllByTestId('thread-timeline-lane-label');
    expect(labels[1]).toHaveTextContent('(no preview)');
    expect(labels[1]).toHaveAttribute('title', rootUuid);
  });
});

describe('ThreadTimelineOverlay hover-jump', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it(`scrolls the matching message into view after ${HOVER_JUMP_DEBOUNCE_MS}ms of hover within the active lane`, async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView as Element['scrollIntoView'];
    const threads = [makeThread(1)];
    const messages = new Map([
      [1, [makeMessage(1, 0, 'msg-a'), makeMessage(1, 1, 'msg-b')]],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
    });
    // React Query schedules its state updates as microtasks; await the dots
    // landing with real timers, then switch to fake timers so the debounce
    // can be driven deterministically without affecting query bookkeeping.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    expect(dots).toHaveLength(2);
    vi.useFakeTimers();

    const dotB = dots.find((d) => d.getAttribute('data-message-uuid') === 'msg-b');
    expect(dotB).toBeDefined();
    fireEvent.mouseEnter(dotB!);
    // Before the debounce elapses, scrollIntoView must NOT have fired.
    act(() => {
      vi.advanceTimersByTime(HOVER_JUMP_DEBOUNCE_MS - 1);
    });
    expect(scrollIntoView).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(scrollIntoView).toHaveBeenCalledTimes(1);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: 'center' });
    // The targeted article is the one whose data-message-uuid matches the dot.
    const target = within(screen.getByTestId('conversation-body')).getByText(
      'msg-b',
    );
    expect(scrollIntoView.mock.instances[0]).toBe(target);
  });

  it('cancels the pending jump when the dot is left before the debounce fires', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView as Element['scrollIntoView'];
    const threads = [makeThread(1)];
    const messages = new Map([[1, [makeMessage(1, 0, 'msg-a')]]]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'msg-a' }],
    });
    const dot = (await screen.findAllByTestId('thread-timeline-dot'))[0];
    vi.useFakeTimers();
    fireEvent.mouseEnter(dot);
    act(() => {
      vi.advanceTimersByTime(100);
    });
    fireEvent.mouseLeave(dot);
    act(() => {
      vi.advanceTimersByTime(HOVER_JUMP_DEBOUNCE_MS);
    });
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it('switches the active thread and defers the scroll until the next frame when hovering a dot in another lane', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView as Element['scrollIntoView'];
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
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        // Only the currently-active lane's article is present, mirroring
        // the live app where the conversation pane only renders the active
        // thread's messages.
        conversationArticles: [{ uuid: 'msg-a' }],
      });
      const dots = await screen.findAllByTestId('thread-timeline-dot');
      const targetDot = dots.find(
        (d) => d.getAttribute('data-message-uuid') === 'msg-b',
      );
      expect(targetDot).toBeDefined();

      vi.useFakeTimers();
      fireEvent.mouseEnter(targetDot!);
      act(() => {
        vi.advanceTimersByTime(HOVER_JUMP_DEBOUNCE_MS);
      });
      // The active thread must have flipped to the target's thread.
      expect(useNavStore.getState().activeThreadId).toBe(2);
      // The scroll is deferred to the next frame; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Re-render with the target lane's article in the DOM and the active
      // thread updated, mirroring the live app's response to the switch.
      vi.useRealTimers();
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
      expect(scrollIntoView).toHaveBeenCalledTimes(1);
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
});

describe('ThreadTimelineOverlay active lane highlight', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('marks the active lane with data-active="true"', async () => {
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
});
