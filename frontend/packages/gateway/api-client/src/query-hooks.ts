import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type UseInfiniteQueryResult,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  CreateLaunchOptionRequest,
  GitBranchesResponse,
  GitRepoResponse,
  LaunchOption,
  LaunchOptionsResponse,
  MessagesResponse,
  NewSessionResponse,
  SendRequest,
  SendResponse,
  SendsResponse,
  SessionsResponse,
  ThreadsResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/wire-gen';
import { appendSessionSend } from './cache';
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
 * flatten `data.pages[].sessions` to recover the full ordered list.
 */
export function useSessionsQuery(
  client: ApiClient,
): UseInfiniteQueryResult<{ pages: SessionsResponse[] }, Error> {
  return useInfiniteQuery({
    queryKey: queryKeys.sessions,
    queryFn: ({ pageParam }) =>
      client.getSessions({
        cursor: pageParam ?? undefined,
        limit: SESSIONS_PAGE_SIZE,
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
    retry: 2,
  });
}

/**
 * Milliseconds a fetched thread tree is considered fresh. Every session visible
 * in the windowed list mounts its own thread query, so rows mounting and
 * unmounting during a scroll would otherwise refetch repeatedly. A short window
 * keeps a row's tree from refetching the instant it scrolls back into view while
 * still letting branch-send invalidation (`useCreateSendMutation`) force a
 * refresh immediately, since invalidation overrides `staleTime`.
 */
const SESSION_THREADS_STALE_TIME = 30_000;

/**
 * A single session's thread tree. Disabled until a real session id is supplied.
 *
 * Both the focused-session query in the workspace and each visible session
 * row's query share the same `sessionThreads(sessionId)` key, so React Query
 * dedupes them into one request per session — no double fetch.
 */
export function useSessionThreadsQuery(
  client: ApiClient,
  sessionId: SessionId | null,
): UseQueryResult<ThreadsResponse> {
  return useQuery({
    queryKey:
      sessionId === null
        ? queryKeys.sessionThreadsNone
        : queryKeys.sessionThreads(sessionId),
    queryFn: () => client.getSessionThreads(sessionId as SessionId),
    enabled: sessionId !== null,
    staleTime: SESSION_THREADS_STALE_TIME,
  });
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
    // A 404 (the session row is gone, e.g. a reaped spawn) will never heal by
    // retrying; surface it immediately so the view falls back to client state.
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
  });
}

/**
 * One level of the working-directory browse (`GET /api/workdir/list`) for the
 * new-session picker. A `null` path lists the server default (`$HOME`).
 *
 * Gated by `enabled` so nothing is fetched until the picker is actually mounted;
 * a `400`/`403` from an invalid or forbidden directory rejects with an
 * `ApiError` the picker renders inline. `retry` is disabled so that error
 * surfaces immediately rather than after backoff.
 */
export function useWorkdirListQuery(
  client: ApiClient,
  path: string | null,
  enabled: boolean,
): UseQueryResult<WorkdirListResponse> {
  return useQuery({
    queryKey: queryKeys.workdirList(path),
    queryFn: () => client.getWorkdirList(path ?? undefined),
    enabled,
    retry: false,
  });
}

/**
 * The user's home directory (the default `GET /api/workdir/list` target),
 * cached for path-abbreviation. Shares the `workdirList(null)` cache key with
 * the picker's initial browse, so it costs no extra request; `staleTime:
 * Infinity` since $HOME does not change during a session.
 */
export function useHomeDirQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<WorkdirListResponse> {
  return useQuery({
    queryKey: queryKeys.workdirList(null),
    queryFn: () => client.getWorkdirList(),
    enabled,
    staleTime: Infinity,
  });
}

/**
 * Recently-used working directories (`GET /api/workdir/recent`) for the
 * new-session picker's "Recent" list. Gated by `enabled` so it only fetches
 * while the picker is mounted.
 */
export function useRecentWorkdirsQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<WorkdirRecentResponse> {
  return useQuery({
    queryKey: queryKeys.workdirRecent,
    queryFn: () => client.getWorkdirRecent(),
    enabled,
  });
}

/**
 * Whether the selected directory is a git repository (`GET /api/workdir/git`),
 * for the new-session picker's worktree option. Cheap (no network), so it runs
 * as soon as a directory is selected — `enabled` gates it on a non-empty path.
 *
 * `retry` is disabled so a non-2xx surfaces immediately: the picker simply
 * hides the worktree option when this errors or reports `repo_root: null`.
 */
export function useGitRepoInfoQuery(
  client: ApiClient,
  path: string | null,
  enabled: boolean,
): UseQueryResult<GitRepoResponse> {
  return useQuery({
    queryKey: queryKeys.gitRepoInfo(path ?? ''),
    queryFn: () => client.getGitRepoInfo(path as string),
    enabled: enabled && path !== null,
    retry: false,
  });
}

/**
 * A repository's remote branches (`GET /api/workdir/git/branches`) for the
 * worktree start-point picker's "other remote branch" list. This performs a
 * `git fetch` server-side, so it is fetched lazily: `enabled` should stay
 * `false` until the user actually opens the remote-branch picker.
 *
 * `retry` is disabled so a `400` (the path is not a git repository) surfaces
 * immediately as an `ApiError` the picker can render inline, rather than after
 * backoff.
 */
export function useGitBranchesQuery(
  client: ApiClient,
  path: string | null,
  enabled: boolean,
): UseQueryResult<GitBranchesResponse> {
  return useQuery({
    queryKey: queryKeys.gitBranches(path ?? ''),
    queryFn: () => client.getGitBranches(path as string),
    enabled: enabled && path !== null,
    retry: false,
  });
}

/**
 * Spawn a brand-new session (`POST /api/sessions`); refresh the session list.
 *
 * Library surface for an explicit "New session" affordance. The current UI does
 * not use it: New starts an empty composer and the first Send (`new_session:
 * true`) spawns the session, so the optimistic pending item reconciles in one
 * round-trip. Likewise `useOpenSessionMutation` exists for an explicit Resume,
 * but the UI resumes a closed session by sending to its main thread (the backend
 * auto-resumes). Only `useCloseSessionMutation` is currently wired (the
 * navigator Close button).
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

/** Close an open session (`POST /api/sessions/{id}/close`). */
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
 * The registered launch options (`GET /api/launch-options`) for the settings
 * screen. Gated by `enabled` so it only fetches while the settings view is
 * mounted; mutations invalidate this key to refresh the list.
 */
export function useLaunchOptionsQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<LaunchOptionsResponse> {
  return useQuery({
    queryKey: queryKeys.launchOptions,
    queryFn: () => client.getLaunchOptions(),
    enabled,
  });
}

/**
 * Register a launch option (`POST /api/launch-options`); refresh the list on
 * success so the new row appears.
 */
export function useCreateLaunchOptionMutation(
  client: ApiClient,
): UseMutationResult<LaunchOption, Error, CreateLaunchOptionRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateLaunchOptionRequest) =>
      client.createLaunchOption(body),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.launchOptions,
      });
    },
  });
}

/**
 * Delete a launch option (`DELETE /api/launch-options/{id}`); refresh the list
 * on success so the removed row disappears.
 */
export function useDeleteLaunchOptionMutation(
  client: ApiClient,
): UseMutationResult<void, Error, number> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => client.deleteLaunchOption(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.launchOptions,
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
