import { describe, expect, it, vi } from 'vitest';
import { QueryClient, QueryObserver } from '@tanstack/react-query';
import type {
  Message,
  MessagesResponse,
  SessionsResponse,
  Thread,
  ThreadsResponse,
} from '@delta/wire-gen';
import {
  invalidateAll,
  invalidateThreadMessages,
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
    thread_id: 't',
    role,
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq,
    content_text: null,
    content: [],
    created_at: '2026-01-01T00:00:00Z',
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
            transcript_path: null,
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
          },
          open: true,
          pane_starting: false,
          hooks_unreachable: false,
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
