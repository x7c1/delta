import type { QueryClient } from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type { Message, MessagesResponse, Send, SendsResponse } from '@delta/wire-gen';
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

/** Mark a single session's open-send list stale so it refetches. */
export function invalidateSessionSends(
  queryClient: QueryClient,
  sessionId: SessionId,
): void {
  void queryClient.invalidateQueries({
    queryKey: queryKeys.sessionSends(sessionId),
  });
}

/**
 * Insert a just-accepted send into its session's cached open-send list (or
 * create the cache entry), de-duplicating by id and keeping submit (id) order.
 * Applied from the `POST /api/sends` response so the chip is render-ready the
 * instant any view mounts the session's send query — no fetch gap — and the
 * follow-up invalidation reconciles against the server.
 */
export function appendSessionSend(
  queryClient: QueryClient,
  sessionId: SessionId,
  send: Send,
): void {
  queryClient.setQueryData<SendsResponse>(
    queryKeys.sessionSends(sessionId),
    (previous) => {
      const withoutDup = (previous?.sends ?? []).filter(
        (existing) => existing.id !== send.id,
      );
      return {
        sends: [...withoutDup, send].sort((a, b) => a.id - b.id),
        // Turn state is server-reported; an optimistic insert learns nothing
        // about it, so keep what the last fetch said (or idle before any
        // fetch) and let the follow-up invalidation reconcile.
        turn: previous?.turn ?? { state: 'idle', send_id: null },
      };
    },
  );
}

/**
 * Drop a session's cached open-send list entirely. Used when the session row
 * itself is gone (a reaped spawn): a refetch would only 404, and the failure
 * chip is rendered from client state instead.
 */
export function removeSessionSends(
  queryClient: QueryClient,
  sessionId: SessionId,
): void {
  queryClient.removeQueries({ queryKey: queryKeys.sessionSends(sessionId) });
}
