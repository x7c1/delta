import { createRef } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { Message, Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
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

function makeMessage(threadId: number, seq: number, uuid: string): Message {
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

describe('ThreadTimelineOverlay collapse toggle', () => {
  beforeEach(() => {
    window.localStorage.clear();
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
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });

  it('truncates a subthread root uuid to 24 chars with full uuid as the title', async () => {
    const rootUuid = 'a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6';
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
    renderOverlay({ threads, messagesByThread: new Map() });
    const labels = await screen.findAllByTestId('thread-timeline-lane-label');
    expect(labels[0]).toHaveTextContent('main');
    expect(labels[0]).toHaveAttribute('title', 'main');
    expect(labels[1]).toHaveTextContent(rootUuid.slice(0, 24));
    expect(labels[1]).toHaveAttribute('title', rootUuid);
  });
});

describe('ThreadTimelineOverlay hover-jump', () => {
  beforeEach(() => {
    window.localStorage.setItem(TIMELINE_EXPANDED_STORAGE_KEY, 'true');
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it(`scrolls the matching message into view after ${HOVER_JUMP_DEBOUNCE_MS}ms of hover`, async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView as Element['scrollIntoView'];
    const threads = [makeThread(1)];
    const messages = new Map([
      [1, [makeMessage(1, 0, 'msg-a'), makeMessage(1, 1, 'msg-b')]],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
    });
    // React Query schedules its state updates as microtasks; await the dots
    // landing with real timers, then switch to fake timers so the debounce
    // can be driven deterministically without affecting query bookkeeping.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    expect(dots).toHaveLength(2);
    vi.useFakeTimers();

    fireEvent.mouseEnter(dots[1]);
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
});

describe('ThreadTimelineOverlay active lane highlight', () => {
  beforeEach(() => {
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
