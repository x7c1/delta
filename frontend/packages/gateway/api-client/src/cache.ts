import type {
  InfiniteData,
  QueryClient,
  QueryKey,
} from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  Send,
  SendsResponse,
  SessionsResponse,
  ThreadsResponse,
} from '@delta/wire-gen';
import { queryKeys } from './query-keys';

/**
 * Cache patchers driven by WebSocket events. They mutate the same query cache
 * entries keyed by {@link queryKeys} so live updates stay consistent with the
 * REST-loaded data. Every refetch here goes through
 * {@link invalidateDiscardingInFlight} so an event that races a query's first
 * fetch is not lost; {@link invalidateSessionsCoalesced} only delays its own.
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
 * Mark every cached query stale so its active observers refetch. Used to
 * resynchronise after the live channel reconnects, when any resource may have
 * changed during the gap.
 *
 * Goes through {@link invalidateDiscardingInFlight} (the empty key matches every
 * query) rather than a bare `invalidateQueries()`: a query whose first fetch is
 * still in flight at reconnect would otherwise keep that fetch's pre-reconnect
 * answer as fresh data.
 */
export function invalidateAll(queryClient: QueryClient): void {
  // The empty key refetches the session list too, which is the refetch a
  // pending coalesced one was waiting to make.
  cancelPendingSessionsRefetch(queryClient);
  invalidateDiscardingInFlight(queryClient, []);
}

/**
 * How long the session list waits for the live events to go quiet before the
 * coalesced refetch ({@link invalidateSessionsCoalesced}) starts. Events come in
 * bursts that are a few milliseconds apart — a turn's end brings
 * `transcript_updated` and `turn_completed` together, and several open sessions
 * each bring their own — so a quarter of a second takes in a whole burst, while
 * staying short enough that the navigator still looks live.
 */
export const SESSIONS_REFETCH_QUIET_MS = 250;

/**
 * The longest the coalesced refetch is put off after the first event it
 * absorbs. A session streaming its transcript produces `transcript_updated`
 * more often than {@link SESSIONS_REFETCH_QUIET_MS}, so the quiet window alone
 * would never close; a second bounds how stale its row's recency and running
 * state can get.
 */
export const SESSIONS_REFETCH_MAX_WAIT_MS = 1_000;

type PendingSessionsRefetch = {
  quiet: ReturnType<typeof setTimeout>;
  deadline: ReturnType<typeof setTimeout>;
};

/**
 * The coalesced session-list refetch each query client has waiting, if any.
 * Keyed by the client it refetches, so the timers live and die with that
 * client: two clients (or two tests) never share one.
 */
const pendingSessionsRefetches = new WeakMap<
  QueryClient,
  PendingSessionsRefetch
>();

function cancelPendingSessionsRefetch(queryClient: QueryClient): void {
  const pending = pendingSessionsRefetches.get(queryClient);
  if (pending === undefined) {
    return;
  }
  clearTimeout(pending.quiet);
  clearTimeout(pending.deadline);
  pendingSessionsRefetches.delete(queryClient);
}

/**
 * Whether a coalesced session-list refetch ({@link invalidateSessionsCoalesced})
 * is waiting on its timer. In that window the list is neither fetching nor
 * current: the rows on hand predate the event, and the answer that reflects it
 * has not been requested yet. A reader that takes "not fetching" to mean "the
 * pages in hand are the latest" (the workspace's focus reconciliation, or a
 * load-more that would append a fresh page to stale ones) must treat this as
 * a fetch still to come.
 *
 * Not reactive on its own, but it never needs to be: it turns on only as the
 * list fetch in flight is cancelled (or while none is), and off only as the
 * refetch starts, and both of those notify the list's observers.
 */
export function hasPendingSessionsRefetch(queryClient: QueryClient): boolean {
  return pendingSessionsRefetches.has(queryClient);
}

/**
 * Mark the session list stale and refetch it at once, superseding any list
 * fetch in flight and any coalesced refetch still waiting (it is the same
 * refetch, so nothing is left on a timer).
 *
 * Used on the lifecycle events (`session_registered`, `session_opened`,
 * `session_closed`, `session_removed`, `spawn_pane_ready`, `spawn_failed`),
 * which move a session's presence, status, open flag or pane: they are rare,
 * and the workspace's focus reconciliation reads `isFetching` right after them,
 * so the refetch must start in the same tick (see
 * {@link invalidateDiscardingInFlight}). Also used after a user action whose
 * response shows the list is stale (a send that launched a session, a refusal
 * naming a lost hook connection or an already-answered prompt): one refetch
 * per action, so there is nothing to coalesce.
 */
export function invalidateSessions(queryClient: QueryClient): void {
  cancelPendingSessionsRefetch(queryClient);
  invalidateDiscardingInFlight(queryClient, queryKeys.sessions);
}

/**
 * Mark the session list stale and discard any list fetch in flight now, but
 * start the refetch only once no further such call has arrived for
 * {@link SESSIONS_REFETCH_QUIET_MS}, and at most
 * {@link SESSIONS_REFETCH_MAX_WAIT_MS} after the first one.
 *
 * Used on the high-frequency events (`transcript_updated`, `turn_started`,
 * `turn_completed`, `turn_interrupted`, `subagent_started`,
 * `subagent_finished`). They arrive in bursts, and each list fetch re-reads
 * every loaded page and every live session's live state on the server —
 * cancelling a request on the client does not stop the server from assembling
 * it — so a burst is answered by one trailing refetch instead of one each.
 *
 * Only the new fetch waits. The in-flight one is discarded at the event,
 * because the server assembled it before the event: landing it would, say,
 * bring back the running spinner a `turn_completed` just cleared (see
 * `useSeedRunningFromSessionList`). Cancelling reverts a query with data to
 * that data (and a first fetch to `pending` with none), so nothing older than
 * the event is applied, and the trailing refetch reads the post-event state.
 */
export function invalidateSessionsCoalesced(queryClient: QueryClient): void {
  const filters = { queryKey: queryKeys.sessions };
  // The cancel reverts the state synchronously, so the invalidation after it
  // is not undone by the revert.
  void queryClient.cancelQueries(filters);
  void queryClient.invalidateQueries({ ...filters, refetchType: 'none' });

  // Whichever timer fires first clears the other along with the entry.
  const refetch = () => invalidateSessions(queryClient);
  const pending = pendingSessionsRefetches.get(queryClient);
  if (pending !== undefined) {
    clearTimeout(pending.quiet);
    pending.quiet = setTimeout(refetch, SESSIONS_REFETCH_QUIET_MS);
    return;
  }
  pendingSessionsRefetches.set(queryClient, {
    quiet: setTimeout(refetch, SESSIONS_REFETCH_QUIET_MS),
    deadline: setTimeout(refetch, SESSIONS_REFETCH_MAX_WAIT_MS),
  });
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

/**
 * Write every listed session's threads (`SessionListItem.threads`) into its
 * `sessionThreads(sessionId)` cache entry, so the navigator rows and the focused
 * workspace read one copy of each tree and the list costs no request per row.
 * `setQueryData` starts no fetch, so seeding requests nothing.
 *
 * `requestedAt` is when the page's request was sent. An entry updated after
 * that (a targeted refetch of the focused session that landed while the page
 * was in flight) is newer than the page's snapshot and is left alone, so a slow
 * list response never rolls a freshly branched tree back.
 */
export function seedSessionThreads(
  queryClient: QueryClient,
  page: SessionsResponse,
  requestedAt: number,
): void {
  for (const item of page.sessions) {
    const queryKey = queryKeys.sessionThreads(item.session.id);
    const updatedAt = queryClient.getQueryState(queryKey)?.dataUpdatedAt ?? 0;
    if (updatedAt > requestedAt) {
      continue;
    }
    queryClient.setQueryData<ThreadsResponse>(queryKey, {
      threads: item.threads,
    });
  }
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
