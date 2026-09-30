import type {
  InfiniteData,
  QueryClient,
  QueryKey,
} from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  Message,
  MessagesResponse,
  Send,
  SendsResponse,
  SessionsResponse,
} from '@delta/wire-gen';
import { queryKeys } from './query-keys';

/**
 * Cache patchers driven by WebSocket events. They mutate the same query cache
 * entries keyed by {@link queryKeys} so live updates stay consistent with the
 * REST-loaded data. Every invalidation here goes through
 * {@link invalidateDiscardingInFlight} so an event that races a query's first
 * fetch is not lost.
 */

/**
 * Mark `queryKey` stale so its active observers refetch, discarding a *first*
 * fetch that is still in flight for it.
 *
 * A plain `invalidateQueries` is not enough for a live event. When the event
 * lands while the query's first fetch is still in flight, TanStack Query
 * ignores `cancelRefetch` (it only cancels a running fetch once the query has
 * data) and hands back the in-flight promise instead of starting a new fetch.
 * That fetch then succeeds with the pre-event server state and its success
 * clears `isInvalidated`, so the stale answer is kept as fresh data, and with a
 * long `staleTime` nothing reads it again. A thread whose transcript was
 * ingested a few milliseconds after its first `GET` was answered would show no
 * messages until some later event happened to invalidate it once more.
 *
 * Such a query is therefore cancelled — which reverts it to `pending` with no
 * data — and invalidated again once the cancel settles, so a fresh fetch reads
 * the post-event state. Every other match is invalidated synchronously, exactly
 * as `invalidateQueries` would: a query that already has data gets its running
 * fetch cancelled and a new one started in the same tick, and callers rely on
 * that (the workspace's focus reconciliation reads `isFetching` right after a
 * lifecycle event to tell "row not listed yet" from "row gone").
 *
 * So do not collapse this into a bare `invalidateQueries` (the event is lost),
 * nor cancel-then-invalidate every match (the refetch starts a microtask late
 * and the focus reconciliation misreads `isFetching`).
 */
function invalidateDiscardingInFlight(
  queryClient: QueryClient,
  queryKey: QueryKey,
): void {
  // Collected before invalidating, while the in-flight fetch is still the
  // first one; `invalidateQueries` below does not change that state.
  const firstFetchesInFlight = queryClient
    .getQueryCache()
    .findAll({ queryKey })
    .filter(
      (query) =>
        query.state.data === undefined && query.state.fetchStatus !== 'idle',
    );
  void queryClient.invalidateQueries({ queryKey });
  for (const query of firstFetchesInFlight) {
    const filters = { queryKey: query.queryKey, exact: true };
    void queryClient
      .cancelQueries(filters)
      .then(() => queryClient.invalidateQueries(filters));
  }
}

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
  invalidateDiscardingInFlight(queryClient, queryKeys.sessions);
}

/**
 * The id of the first session in the cached list that is not `excluded`, or
 * `null` when the cache holds no other session (or has not loaded yet).
 *
 * Read for the focus handoff when the focused session stops existing
 * (`session_removed`): the cached list is already in the list's own order — live
 * sessions first, then the closed ones, each group most-recently-active first —
 * so its head is "the next session the user would have picked". Reading the
 * cache makes the choice synchronous and deterministic; it must therefore happen
 * *before* {@link invalidateSessions}, not after, or it would race a refetch.
 */
export function firstOtherSessionId(
  queryClient: QueryClient,
  excluded: SessionId,
): SessionId | null {
  const cached = queryClient.getQueryData<InfiniteData<SessionsResponse>>(
    queryKeys.sessions,
  );
  for (const page of cached?.pages ?? []) {
    for (const item of page.sessions) {
      if (item.session.id !== excluded) {
        return item.session.id;
      }
    }
  }
  return null;
}

/**
 * Mark the repository list and both PR lenses stale so they refetch.
 *
 * Used when a repository clone lands: whether a clone exists is exactly what
 * `has_local_clone` on a PR row and the clone rows of the repository list
 * report, so both are wrong until they re-read. Both lenses are invalidated
 * because a newly-cloned repository can own PRs under either.
 */
export function invalidateRepositoriesAndPullRequests(
  queryClient: QueryClient,
): void {
  invalidateDiscardingInFlight(queryClient, queryKeys.repositories);
  invalidateDiscardingInFlight(queryClient, queryKeys.pullRequests('reviewer'));
  invalidateDiscardingInFlight(queryClient, queryKeys.pullRequests('author'));
}

/** Mark a single session's thread tree stale so it refetches. */
export function invalidateSessionThreads(
  queryClient: QueryClient,
  sessionId: SessionId,
): void {
  invalidateDiscardingInFlight(queryClient, queryKeys.sessionThreads(sessionId));
}

/** Mark a single thread's transcript stale so it refetches. */
export function invalidateThreadMessages(
  queryClient: QueryClient,
  threadId: ThreadId,
): void {
  invalidateDiscardingInFlight(queryClient, queryKeys.messages(threadId));
}

/** Mark a single session's open-send list stale so it refetches. */
export function invalidateSessionSends(
  queryClient: QueryClient,
  sessionId: SessionId,
): void {
  invalidateDiscardingInFlight(queryClient, queryKeys.sessionSends(sessionId));
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
        // The live state (turn phase, the pending permission queue's head and
        // depth, the pending question, running subagents) is server-reported; an
        // optimistic insert learns nothing about it, so keep what the last fetch
        // said (or idle/none/empty before any fetch) and let the follow-up
        // invalidation reconcile.
        turn: previous?.turn ?? { state: 'idle', send_id: null, thread_id: null },
        permission: previous?.permission ?? null,
        permission_count: previous?.permission_count ?? 0,
        question: previous?.question ?? null,
        running_subagents: previous?.running_subagents ?? [],
      };
    },
  );
}

/**
 * Drop a session's cached open-send list entirely. Used when the session
 * row itself is gone (`session_removed`): a refetch would only 404.
 */
export function removeSessionSends(
  queryClient: QueryClient,
  sessionId: SessionId,
): void {
  queryClient.removeQueries({ queryKey: queryKeys.sessionSends(sessionId) });
}
