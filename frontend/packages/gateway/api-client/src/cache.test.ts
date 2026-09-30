import { describe, expect, it, vi } from 'vitest';
import { QueryClient, QueryObserver } from '@tanstack/react-query';
import type { Message, MessagesResponse } from '@delta/wire-gen';
import { invalidateThreadMessages } from './cache';
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
