import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  InfiniteQueryObserver,
  QueryClient,
  QueryObserver,
} from '@tanstack/react-query';
import type {
  Message,
  MessagesResponse,
  SessionsResponse,
  Thread,
  ThreadsResponse,
} from '@delta/wire-gen';
import {
  hasPendingSessionsRefetch,
  invalidateAll,
  invalidateSessions,
  invalidateSessionsCoalesced,
  invalidateThreadMessages,
  SESSIONS_REFETCH_MAX_WAIT_MS,
  SESSIONS_REFETCH_QUIET_MS,
  seedSessionThreads,
} from './cache';
import { queryKeys } from './query-keys';

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function message(uuid: string, role: Message['role'], seq: number): Message {
  return {
    uuid,
    session_id: 's',
    thread_id: 1,
    role,
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
    provider_item_id: null,
  };
}

describe('invalidateThreadMessages', () => {
  it('refetches when a live event lands during the first fetch', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: 30_000 } },
    });
    const threadId = 5;
    // Each call to the queryFn gets its own answer the test resolves by hand:
    // the first one is the server state before ingest, the second after.
    const answers: Deferred<MessagesResponse>[] = [];
    const observer = new QueryObserver<MessagesResponse>(queryClient, {
      queryKey: queryKeys.messages(threadId),
      queryFn: () => {
        const answer = deferred<MessagesResponse>();
        answers.push(answer);
        return answer.promise;
      },
    });
    const unsubscribe = observer.subscribe(() => {});

    // The first fetch is in flight; the transcript is ingested and the live
    // event invalidates the thread before that fetch is answered.
    expect(answers).toHaveLength(1);
    invalidateThreadMessages(queryClient, threadId);
    answers[0].resolve({ messages: [] });

    await vi.waitFor(() => expect(answers).toHaveLength(2));
    const ingested = [
      message('u1', 'user', 1),
      message('a1', 'assistant', 2),
    ];
    answers[1].resolve({ messages: ingested });

    await vi.waitFor(() =>
      expect(observer.getCurrentResult().status).toBe('success'),
    );
    expect(observer.getCurrentResult().data).toEqual({ messages: ingested });
    expect(
      queryClient.getQueryData<MessagesResponse>(queryKeys.messages(threadId)),
    ).toEqual({ messages: ingested });

    unsubscribe();
    queryClient.clear();
  });

  it('starts the refetch of a query that has data in the same tick', () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: 30_000 } },
    });
    const threadId = 5;
    queryClient.setQueryData<MessagesResponse>(queryKeys.messages(threadId), {
      messages: [],
    });
    const observer = new QueryObserver<MessagesResponse>(queryClient, {
      queryKey: queryKeys.messages(threadId),
      queryFn: () => new Promise<MessagesResponse>(() => {}),
    });
    const unsubscribe = observer.subscribe(() => {});
    expect(observer.getCurrentResult().isFetching).toBe(false);

    // Callers read `isFetching` right after the event to tell "not listed
    // yet" from "gone", so the refetch must not wait for a microtask.
    invalidateThreadMessages(queryClient, threadId);
    expect(observer.getCurrentResult().isFetching).toBe(true);

    unsubscribe();
    queryClient.clear();
  });
});

describe('invalidateAll', () => {
  it('refetches a query whose first fetch is in flight at reconnect', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: 30_000 } },
    });
    const threadId = 5;
    const answers: Deferred<MessagesResponse>[] = [];
    const observer = new QueryObserver<MessagesResponse>(queryClient, {
      queryKey: queryKeys.messages(threadId),
      queryFn: () => {
        const answer = deferred<MessagesResponse>();
        answers.push(answer);
        return answer.promise;
      },
    });
    const unsubscribe = observer.subscribe(() => {});

    // The thread was opened while the channel was down; its first fetch is
    // still in flight when the channel reconnects and resyncs everything.
    expect(answers).toHaveLength(1);
    invalidateAll(queryClient);
    answers[0].resolve({ messages: [] });

    await vi.waitFor(() => expect(answers).toHaveLength(2));
    const changed = [message('u1', 'user', 1)];
    answers[1].resolve({ messages: changed });

    await vi.waitFor(() =>
      expect(observer.getCurrentResult().status).toBe('success'),
    );
    expect(observer.getCurrentResult().data).toEqual({ messages: changed });

    unsubscribe();
    queryClient.clear();
  });
});

describe('invalidateSessionsCoalesced', () => {
  type SessionsData = { pages: SessionsResponse[]; pageParams: unknown[] };

  function listPage(title: string): SessionsResponse {
    return {
      sessions: [
        {
          session: {
            id: 's',
            cwd: '/work',
            transcript_path: '/work/transcript.jsonl',
            title,
            status: 'active',
            created_at: '2026-01-01T00:00:00Z',
            branch_at_launch: null,
            repo_root: null,
            repository_display_name: null,
            provider: 'claude',
            provider_session_id: null,
            provider_thread_id: null,
            pull_request_number: null,
            failure_reason: null,
            closed_reason: null,
          },
          open: true,
          pane_starting: false,
          hooks_unreachable: false,
          turn: { state: 'idle', send_id: null, thread_id: null },
          running_subagents: [],
          main_thread_id: 1,
          threads: [],
          last_activity_at: null,
        },
      ],
      next_cursor: null,
    };
  }

  /**
   * A query client whose session list is watched, as the navigator holds it,
   * with its first fetch in flight. Every list request is recorded and answered
   * by hand.
   */
  function listObserver() {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: 30_000 } },
    });
    const requests: Deferred<SessionsResponse>[] = [];
    const observer = new InfiniteQueryObserver<
      SessionsResponse,
      Error,
      SessionsData,
      typeof queryKeys.sessions,
      string | null
    >(queryClient, {
      queryKey: queryKeys.sessions,
      queryFn: () => {
        const answer = deferred<SessionsResponse>();
        requests.push(answer);
        return answer.promise;
      },
      initialPageParam: null,
      getNextPageParam: () => undefined,
    });
    const unsubscribe = observer.subscribe(() => {});
    const titles = () =>
      observer
        .getCurrentResult()
        .data?.pages.flatMap((p) => p.sessions.map((i) => i.session.title));
    return {
      queryClient,
      observer,
      requests,
      titles,
      dispose: () => {
        unsubscribe();
        queryClient.clear();
      },
    };
  }

  /**
   * {@link listObserver} with the first fetch answered (titled `loaded`) and
   * dropped from `requests`, so `requests` holds only the refetches.
   */
  async function watchedList() {
    const list = listObserver();
    list.requests[0].resolve(listPage('loaded'));
    await vi.waitFor(() =>
      expect(list.observer.getCurrentResult().status).toBe('success'),
    );
    list.requests.shift();
    return list;
  }

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('coalesces a burst of invalidations into one request issued after the quiet window', async () => {
    const { queryClient, observer, requests, dispose } = await watchedList();

    // A turn's end and the transcript lines it wrote, plus another session's.
    invalidateSessionsCoalesced(queryClient);
    await vi.advanceTimersByTimeAsync(50);
    invalidateSessionsCoalesced(queryClient);
    await vi.advanceTimersByTimeAsync(50);
    invalidateSessionsCoalesced(queryClient);

    // Stale at once, but nothing is requested while the burst goes on.
    expect(queryClient.getQueryState(queryKeys.sessions)?.isInvalidated).toBe(
      true,
    );
    expect(observer.getCurrentResult().isFetching).toBe(false);
    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_QUIET_MS - 1);
    expect(requests).toHaveLength(0);

    // The window closes after the last one: exactly one request goes out.
    await vi.advanceTimersByTimeAsync(1);
    expect(requests).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_MAX_WAIT_MS);
    expect(requests).toHaveLength(1);

    dispose();
  });

  it('discards a list fetch in flight at the event instead of applying it', async () => {
    const { queryClient, observer, requests, titles, dispose } =
      await watchedList();

    // A refetch goes out and is assembled from the server's state before the
    // event, then the event lands while it is still on the wire.
    void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    expect(requests).toHaveLength(1);
    const stale = requests.shift()!;
    invalidateSessionsCoalesced(queryClient);
    expect(observer.getCurrentResult().isFetching).toBe(false);

    stale.resolve(listPage('before the event'));
    await vi.advanceTimersByTimeAsync(0);
    expect(titles()).toEqual(['loaded']);

    // The trailing refetch reads the post-event state, and that one lands.
    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_QUIET_MS);
    expect(requests).toHaveLength(1);
    requests[0].resolve(listPage('after the event'));
    await vi.advanceTimersByTimeAsync(0);
    expect(titles()).toEqual(['after the event']);

    dispose();
  });

  it('discards a first fetch in flight at the event and fetches again after the window', async () => {
    const { queryClient, observer, requests, titles, dispose } =
      listObserver();

    // The list's first fetch is still in flight when the event lands.
    expect(requests).toHaveLength(1);
    invalidateSessionsCoalesced(queryClient);
    requests[0].resolve(listPage('before the event'));
    await vi.advanceTimersByTimeAsync(0);
    expect(observer.getCurrentResult().data).toBeUndefined();

    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_QUIET_MS);
    expect(requests).toHaveLength(2);
    requests[1].resolve(listPage('after the event'));
    await vi.advanceTimersByTimeAsync(0);
    expect(titles()).toEqual(['after the event']);

    dispose();
  });

  it('refetches once the maximum wait has passed under a steady stream of events', async () => {
    const { queryClient, requests, dispose } = await watchedList();

    // A streaming session: events closer together than the quiet window, so
    // the window alone would never close.
    const interval = SESSIONS_REFETCH_QUIET_MS / 2;
    let elapsed = 0;
    invalidateSessionsCoalesced(queryClient);
    while (elapsed + interval < SESSIONS_REFETCH_MAX_WAIT_MS) {
      await vi.advanceTimersByTimeAsync(interval);
      elapsed += interval;
      invalidateSessionsCoalesced(queryClient);
    }
    expect(requests).toHaveLength(0);

    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_MAX_WAIT_MS - elapsed);
    expect(requests).toHaveLength(1);

    // The stream goes on; the next refetch is bounded the same way.
    invalidateSessionsCoalesced(queryClient);
    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_QUIET_MS);
    expect(requests).toHaveLength(2);

    dispose();
  });

  it('runs a pending coalesced refetch at once on an immediate invalidation and leaves none pending', async () => {
    const { queryClient, observer, requests, dispose } = await watchedList();

    invalidateSessionsCoalesced(queryClient);
    expect(requests).toHaveLength(0);
    expect(hasPendingSessionsRefetch(queryClient)).toBe(true);

    // A lifecycle event: the refetch starts in the same tick, for callers
    // that read `isFetching` right after it.
    invalidateSessions(queryClient);
    expect(observer.getCurrentResult().isFetching).toBe(true);
    expect(requests).toHaveLength(1);

    // It was the coalesced refetch too: no timer fires another one.
    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_MAX_WAIT_MS * 2);
    expect(requests).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
    expect(hasPendingSessionsRefetch(queryClient)).toBe(false);

    dispose();
  });

  it('keeps the timers of each query client to itself', async () => {
    const first = await watchedList();
    const second = await watchedList();

    invalidateSessionsCoalesced(first.queryClient);
    invalidateSessions(second.queryClient);
    expect(second.requests).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(SESSIONS_REFETCH_QUIET_MS);
    expect(first.requests).toHaveLength(1);
    expect(second.requests).toHaveLength(1);

    first.dispose();
    second.dispose();
  });
});

describe('seedSessionThreads', () => {
  function thread(id: number, sessionId: string): Thread {
    return {
      id,
      session_id: sessionId,
      title: id === 1 ? 'main' : `branch ${id}`,
      parent_thread_id: id === 1 ? null : 1,
      root_message_uuid: null,
      created_at: '2026-01-01T00:00:00Z',
      last_activity_at: null,
    };
  }

  function page(sessionId: string, threads: Thread[]): SessionsResponse {
    return {
      sessions: [
        {
          session: {
            id: sessionId,
            cwd: '/work',
            transcript_path: '/work/transcript.jsonl',
            title: null,
            status: 'active',
            created_at: '2026-01-01T00:00:00Z',
            branch_at_launch: null,
            repo_root: null,
            repository_display_name: null,
            provider: 'claude',
            provider_session_id: null,
            provider_thread_id: null,
            pull_request_number: null,
            failure_reason: null,
            closed_reason: null,
          },
          open: true,
          pane_starting: false,
          hooks_unreachable: false,
          turn: { state: 'idle', send_id: null, thread_id: null },
          running_subagents: [],
          main_thread_id: 1,
          threads,
          last_activity_at: null,
        },
      ],
      next_cursor: null,
    };
  }

  it("writes each listed session's threads into its thread cache without fetching", () => {
    const queryClient = new QueryClient();
    const queryFn = vi.fn();
    // A passive observer, as a navigator row holds one.
    const observer = new QueryObserver<ThreadsResponse>(queryClient, {
      queryKey: queryKeys.sessionThreads('s'),
      queryFn,
      enabled: false,
    });
    const unsubscribe = observer.subscribe(() => {});

    seedSessionThreads(
      queryClient,
      page('s', [thread(1, 's'), thread(2, 's')]),
      Date.now(),
    );

    expect(observer.getCurrentResult().data?.threads.map((t) => t.id)).toEqual(
      [1, 2],
    );
    expect(queryFn).not.toHaveBeenCalled();
    unsubscribe();
  });

  it('keeps an entry updated after the page was requested', () => {
    const queryClient = new QueryClient();
    const requestedAt = Date.now() - 1_000;
    // The focused session's targeted refetch landed while the page was in
    // flight: it already holds the branch the page's snapshot predates.
    queryClient.setQueryData<ThreadsResponse>(queryKeys.sessionThreads('s'), {
      threads: [thread(1, 's'), thread(2, 's')],
    });

    seedSessionThreads(queryClient, page('s', [thread(1, 's')]), requestedAt);

    expect(
      queryClient
        .getQueryData<ThreadsResponse>(queryKeys.sessionThreads('s'))
        ?.threads.map((t) => t.id),
    ).toEqual([1, 2]);
  });

  it('replaces an entry older than the page', () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData<ThreadsResponse>(
      queryKeys.sessionThreads('s'),
      { threads: [thread(1, 's')] },
      { updatedAt: Date.now() - 1_000 },
    );

    seedSessionThreads(
      queryClient,
      page('s', [thread(1, 's'), thread(2, 's')]),
      Date.now(),
    );

    expect(
      queryClient
        .getQueryData<ThreadsResponse>(queryKeys.sessionThreads('s'))
        ?.threads.map((t) => t.id),
    ).toEqual([1, 2]);
  });
});
