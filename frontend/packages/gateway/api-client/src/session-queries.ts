import {
  skipToken,
  useInfiniteQuery,
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
  type UseInfiniteQueryResult,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  MessagesResponse,
  NewSessionResponse,
  SendRequest,
  SendResponse,
  SendsResponse,
  SessionsResponse,
  Thread,
  ThreadsResponse,
} from '@delta/wire-gen';
import { appendSessionSend, seedSessionThreads } from './cache';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

/** Sessions per page requested from `GET /api/sessions`. */
const SESSIONS_PAGE_SIZE = 30;

/**
 * The session list (`GET /api/sessions`), cursor-paginated as an infinite query.
 * Runs on app load and is invalidated by lifecycle events
 * (`session_registered`/`session_opened`/`session_closed`). A bounded retry
 * covers a transient failure while the server is still coming up.
 *
 * Pages are addressed by the server's opaque `next_cursor`: the first page sends
 * no cursor (`pageParam: null`), and each subsequent page echoes back the prior
 * page's `next_cursor`. `getNextPageParam` returns `undefined` at end-of-list
 * (`next_cursor: null`), which is how `hasNextPage` becomes `false`. Consumers
 * flatten `data.pages[].sessions` to recover the full ordered list — the server
 * orders it open-first, so page 1 leads with every live session and the pages
 * concatenate straight into the display order. The `session_opened` /
 * `session_closed` invalidations above are what move a session between the two
 * groups.
 */
export function useSessionsQuery(
  client: ApiClient,
): UseInfiniteQueryResult<{ pages: SessionsResponse[] }, Error> {
  const queryClient = useQueryClient();
  return useInfiniteQuery({
    queryKey: queryKeys.sessions,
    queryFn: async ({ pageParam }) => {
      const requestedAt = Date.now();
      const page = await client.getSessions({
        cursor: pageParam ?? undefined,
        limit: SESSIONS_PAGE_SIZE,
      });
      // Each item carries its session's threads: hand them to the per-session
      // thread cache the rows and the focused workspace read, so the list
      // costs no request per row (see `seedSessionThreads`).
      seedSessionThreads(queryClient, page, requestedAt);
      return page;
    },
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
    retry: 2,
  });
}

/**
 * Milliseconds a thread tree in the cache is considered fresh. Applies to the
 * focused workspace's {@link useSessionThreadsQuery}: an entry seeded from the
 * session list (or fetched) within this window is served as is when a session
 * gains focus, rather than refetched the instant it does. Branch-send and
 * focused-turn invalidations still force a refresh immediately, since
 * invalidation overrides `staleTime`.
 */
const SESSION_THREADS_STALE_TIME = 30_000;

/**
 * Milliseconds a fetched thread's messages are considered fresh. Cross-lane
 * jumps in the timeline revisit threads whose messages are already cached;
 * without a stale window each revisit triggers a background refetch whose new
 * array reference cascades through downstream `useMemo`s. WS-driven
 * invalidation (`invalidateThreadMessages`, triggered by session events) still
 * forces an immediate refresh because invalidation overrides `staleTime`, and
 * it discards a first fetch still in flight, so realtime freshness is
 * preserved even when the event races the thread's initial load.
 */
const MESSAGES_STALE_TIME = 30_000;

/** The shared options of every observer of a session's thread tree. */
function sessionThreadsQueryOptions(client: ApiClient, sessionId: SessionId) {
  return {
    queryKey: queryKeys.sessionThreads(sessionId),
    // Kept on the passive observers too (see `useListedSessionThreads`): a
    // refetch an invalidation starts runs with the options the query last
    // received, which may be theirs.
    queryFn: () => client.getSessionThreads(sessionId),
    staleTime: SESSION_THREADS_STALE_TIME,
  };
}

/**
 * The focused session's thread tree (`GET /api/sessions/{id}/threads`).
 * Disabled until a real session id is supplied.
 *
 * This is the one observer that fetches the tree: the session list seeds the
 * same `sessionThreads(sessionId)` entry for every session it carries, and the
 * navigator rows read it passively ({@link useListedSessionThreads}). Branch
 * sends and the focused session's turn events invalidate the entry, and this
 * observer is what turns that into a targeted refetch — which every row of the
 * session then shows.
 */
export function useSessionThreadsQuery(
  client: ApiClient,
  sessionId: SessionId | null,
): UseQueryResult<ThreadsResponse> {
  return useQuery(
    sessionId === null
      ? { queryKey: queryKeys.sessionThreadsNone, queryFn: skipToken }
      : sessionThreadsQueryOptions(client, sessionId),
  );
}

/**
 * A listed session's thread tree, read without ever requesting it.
 *
 * Reads the `sessionThreads(sessionId)` cache entry — seeded from each session
 * list page as it arrives, and refetched while the session is focused by
 * {@link useSessionThreadsQuery} — through a disabled observer: mounting it
 * (a row scrolling into view) and invalidating the entry while only rows
 * observe it start no request. So a row shows its tree from the list alone,
 * and the focused session's targeted refetch (after a branch send, on a turn
 * event) reaches the row the moment it lands.
 *
 * `listed` is the list item's own copy, served while the entry is absent — the
 * cache collected it while nothing observed it, or the row was rendered from a
 * list that did not pass through {@link useSessionsQuery}. It is the very data
 * seeding writes, so the two never tell different stories.
 */
export function useListedSessionThreads(
  client: ApiClient,
  sessionId: SessionId,
  listed: Thread[],
): Thread[] {
  const query = useQuery({
    ...sessionThreadsQueryOptions(client, sessionId),
    enabled: false,
  });
  return query.data?.threads ?? listed;
}

/**
 * A single session's open (non-terminal) sends — the server-side truth behind
 * the pending-send strip. Disabled until a real session id is supplied.
 *
 * Freshness is event-driven: send-affecting session events (turn lifecycle,
 * transcript growth, spawn failure, close) invalidate this key, and a
 * `POST /api/sends` response is patched in via {@link appendSessionSend}, so
 * no `staleTime` tuning is needed.
 */
export function useSessionSendsQuery(
  client: ApiClient,
  sessionId: SessionId | null,
): UseQueryResult<SendsResponse> {
  return useQuery({
    queryKey:
      sessionId === null
        ? queryKeys.sessionSendsNone
        : queryKeys.sessionSends(sessionId),
    queryFn: () => client.getSessionSends(sessionId as SessionId),
    enabled: sessionId !== null,
    // A 404 (the session row is gone, e.g. a removed session) will never heal
    // by retrying; surface it immediately so the view falls back to client
    // state.
    retry: false,
  });
}

export function useThreadMessagesQuery(
  client: ApiClient,
  threadId: ThreadId | null,
): UseQueryResult<MessagesResponse> {
  return useQuery({
    queryKey:
      threadId === null ? queryKeys.messagesNone : queryKeys.messages(threadId),
    queryFn: () => client.getThreadMessages(threadId as ThreadId),
    enabled: threadId !== null,
    staleTime: MESSAGES_STALE_TIME,
  });
}

/** One thread's messages query result, keyed by the thread id it ran for. */
export interface ThreadMessagesQueryEntry {
  threadId: ThreadId;
  result: UseQueryResult<MessagesResponse>;
}

/**
 * Messages for several threads at once, one query per thread. Shares the
 * `messages(threadId)` cache key with {@link useThreadMessagesQuery}, so the
 * active thread's already-fetched messages are reused rather than re-requested.
 *
 * Used by the timeline footer to drive its swim-lane dots: the footer needs
 * the message counts for every (sub)thread, not just the focused one. N+1 is
 * acceptable in MVP — a dedicated `all_threads=true` REST is intentionally
 * left for a later pass once usage shows the per-thread fan-out matters.
 *
 * `options.enabled` (default `true`, backward-compatible) gates the fan-out so
 * callers can keep the hook mounted but suppress the per-thread fetches while
 * their UI is hidden. At cold load the browser caps at six HTTP/1.1
 * connections per host, so an always-on fan-out across many threads saturates
 * the pool and stretches the focused thread's `useThreadMessagesQuery` behind
 * it. Disabling here while the timeline is collapsed leaves the focused
 * query untouched (it has its own `enabled` gate keyed on a non-null thread
 * id), and because both hooks share `queryKeys.messages(threadId)` with the
 * same `staleTime`, expanding the timeline later still reuses any messages
 * the focused query has already pulled into cache.
 */
export function useThreadsMessagesQueries(
  client: ApiClient,
  threadIds: ThreadId[],
  options?: { enabled?: boolean },
): ThreadMessagesQueryEntry[] {
  const enabled = options?.enabled ?? true;
  const results = useQueries({
    queries: threadIds.map((threadId) => ({
      queryKey: queryKeys.messages(threadId),
      queryFn: () => client.getThreadMessages(threadId),
      staleTime: MESSAGES_STALE_TIME,
      enabled,
    })),
  });
  return results.map((result, index) => ({
    threadId: threadIds[index],
    result,
  }));
}

/**
 * Spawn a brand-new session (`POST /api/sessions`); refresh the session list.
 *
 * Library surface for an explicit "New session" affordance. The current UI does
 * not use it: New starts an empty composer and the first Send (`new_session:
 * true`) spawns the session, so the optimistic pending item reconciles in one
 * round-trip. Likewise `useOpenSessionMutation` exists for an explicit Resume,
 * but the UI resumes a closed session by sending to its main thread (the backend
 * auto-resumes). The wired ones are `useCloseSessionMutation` and
 * `useDeleteSessionMutation` — the navigator's `Close` and `Remove` items.
 */
export function useNewSessionMutation(
  client: ApiClient,
): UseMutationResult<NewSessionResponse, Error, void> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => client.newSession(),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

/** Resume a closed session (`POST /api/sessions/{id}/open`). */
export function useOpenSessionMutation(
  client: ApiClient,
): UseMutationResult<void, Error, SessionId> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: SessionId) => client.openSession(sessionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

/**
 * Close a session, or cancel a still-starting session's launch
 * (`POST /api/sessions/{id}/close`).
 */
export function useCloseSessionMutation(
  client: ApiClient,
): UseMutationResult<void, Error, SessionId> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: SessionId) => client.closeSession(sessionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

/**
 * Remove a closed session from the list (`DELETE /api/sessions/{id}`).
 *
 * The server broadcasts `session_removed`, which every other tab applies; this
 * invalidation is what spares the acting tab from waiting on the socket for a
 * list it can refetch now.
 */
export function useDeleteSessionMutation(
  client: ApiClient,
): UseMutationResult<void, Error, SessionId> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: SessionId) => client.deleteSession(sessionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

/**
 * Cancel a still-queued send (`POST /api/sends/{id}/cancel`); refresh the
 * session's open-send list so the cancelled chip disappears.
 *
 * The mutation carries the owning `sessionId` alongside the `sendId` so the
 * exact open-send query can be invalidated. A `409` (`send_not_cancellable`)
 * still invalidates: the send already left the queue, so the refetch reconciles
 * the strip either way. Error *presentation* is the call site's job: this
 * gateway hook does not know about the app's notification store, so callers
 * pass an `onError` to `mutate` (see `PendingQueue`) — a refused cancel must
 * surface as an explained refusal, not a dead button.
 */
export function useCancelSendMutation(
  client: ApiClient,
): UseMutationResult<void, Error, { sendId: number; sessionId: SessionId }> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ sendId }: { sendId: number; sessionId: SessionId }) =>
      client.cancelSend(sendId),
    onSettled: (_data, _error, { sessionId }) => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.sessionSends(sessionId),
      });
    },
  });
}

/**
 * Release a held send — one recovered at boot, or parked by the echo
 * deadline — into the normal queued flow (`POST /api/sends/{id}/release`);
 * refresh the session's open-send list so the chip's held label gives way to
 * the row's next truthful state (dispatched, or plain queued when the session
 * is closed or busy).
 *
 * The mutation carries the owning `sessionId` alongside the `sendId` so the
 * exact open-send query can be invalidated. A `409` (`send_not_releasable`)
 * still invalidates: the send already left the releasable window (released,
 * cancelled), so the refetch reconciles the strip either way. Error
 * *presentation* is the call site's job, exactly as with
 * {@link useCancelSendMutation}: callers pass an `onError` to `mutate` (see
 * `PendingQueue`) — a refused release must surface as an explained refusal,
 * not a dead button.
 */
export function useReleaseSendMutation(
  client: ApiClient,
): UseMutationResult<void, Error, { sendId: number; sessionId: SessionId }> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ sendId }: { sendId: number; sessionId: SessionId }) =>
      client.releaseSend(sendId),
    onSettled: (_data, _error, { sessionId }) => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.sessionSends(sessionId),
      });
    },
  });
}

export function useCreateSendMutation(
  client: ApiClient,
): UseMutationResult<SendResponse, Error, SendRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: SendRequest) => client.createSend(body),
    onSuccess: ({ send }) => {
      // A branch send creates a child thread server-side, and a new-session
      // send eventually adds a session; refresh the session list so the tree
      // picks up the change.
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      // Refresh the affected session's thread tree so a freshly-branched
      // child appears immediately. Without this the new thread is absent from
      // the cached list, and the workspace reverts the active thread back to
      // main instead of drilling into the new branch. Every send carries a
      // real session id (a new-session send returns the eagerly-created row's
      // ids).
      void queryClient.invalidateQueries({
        queryKey: queryKeys.sessionThreads(send.session_id),
      });
      // Patch the accepted send straight into its session's open-send cache so
      // the pending chip renders without waiting for a refetch.
      appendSessionSend(queryClient, send.session_id, send);
      void queryClient.invalidateQueries({
        queryKey: queryKeys.sessionSends(send.session_id),
      });
    },
  });
}
