import type { QueryClient } from '@tanstack/react-query';
import type {
  Message,
  MessagesResponse,
  SessionId,
  ThreadId,
} from '@delta/model';
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

/**
 * Mark the session list stale so it refetches. Used on lifecycle events
 * (`session_registered`/`session_opened`/`session_closed`) so a newly-spawned,
 * resumed, or closed session's open flag and presence stay in sync with the UI.
 */
export function invalidateSessions(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
}

/** Mark a single session's thread tree stale so it refetches. */
export function invalidateSessionThreads(
  queryClient: QueryClient,
  sessionId: SessionId,
): void {
  void queryClient.invalidateQueries({
    queryKey: queryKeys.sessionThreads(sessionId),
  });
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
