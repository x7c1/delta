import type { QueryClient } from '@tanstack/react-query';
import type { Message, MessagesResponse, ThreadId } from '@delta/model';
import { queryKeys } from './query-keys';

/**
 * Cache patchers driven by WebSocket events. They mutate the same query cache
 * entries keyed by {@link queryKeys} so live updates stay consistent with the
 * REST-loaded data.
 */

/**
 * Append a message to a thread's cached transcript, de-duplicating by uuid and
 * keeping the list ordered by `seq`. Used to apply incremental transcript
 * growth that arrives via the live channel. No-op if the thread is not cached.
 */
export function appendMessage(
  queryClient: QueryClient,
  threadId: ThreadId,
  message: Message,
): void {
  queryClient.setQueryData<MessagesResponse>(
    queryKeys.messages(threadId),
    (previous) => {
      if (!previous) {
        return previous;
      }
      const withoutDup = previous.messages.filter(
        (existing) => existing.uuid !== message.uuid,
      );
      const messages = [...withoutDup, message].sort((a, b) => a.seq - b.seq);
      return { messages };
    },
  );
}

/** Mark the session/threads queries stale so they refetch from the server. */
export function invalidateThreads(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.threads });
}

/**
 * Mark the session query stale so it refetches from the server. Used when the
 * first message registers the session: `GET /api/session` was 404 before the
 * session row existed, so the cached query is errored — invalidating it lets
 * the UI transition out of the no-session bootstrap state automatically.
 */
export function invalidateSession(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.session });
}

/** Mark a single thread's transcript stale so it refetches. */
export function invalidateThreadMessages(
  queryClient: QueryClient,
  threadId: ThreadId,
): void {
  void queryClient.invalidateQueries({
    queryKey: queryKeys.messages(threadId),
  });
}
