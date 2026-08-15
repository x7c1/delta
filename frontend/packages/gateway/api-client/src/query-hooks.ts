import {
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
  CloneRepositoryRequest,
  CreateLaunchOptionRequest,
  CreateCloneRootRequest,
  GitBranchesResponse,
  GitRepoResponse,
  LaunchOption,
  LaunchOptionsResponse,
  MessagesResponse,
  NewSessionResponse,
  OpenCwdRequest,
  ProvidersResponse,
  PullRequestsResponse,
  RepositoriesResponse,
  CloneRoot,
  CloneRootsResponse,
  SendRequest,
  SendResponse,
  SendsResponse,
  SessionsResponse,
  ThreadsResponse,
  UpdateLaunchOptionRequest,
  VersionResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/wire-gen';
import { appendSessionSend } from './cache';
import type { ApiClient, PullRequestLens } from './http';
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
 * Milliseconds a fetched thread's messages are considered fresh. Cross-lane
 * jumps in the timeline revisit threads whose messages are already cached;
 * without a stale window each revisit triggers a background refetch whose new
 * array reference cascades through downstream `useMemo`s. WS-driven
 * invalidation (`invalidateThreadMessages`, triggered by session events) still
 * forces an immediate refresh because `invalidateQueries` overrides
 * `staleTime`, so realtime freshness is preserved.
 */
const MESSAGES_STALE_TIME = 30_000;

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
 * Registered repositories for the new-session Repository tab
 * (`GET /api/repositories`), most-recently-active first. Gated by `enabled`
 * so it only fetches while the tab is mounted.
 *
 * `staleTime: 0` is the New-session-screen lifetime cache policy: the
 * repository list (which unions session-derived clones with the clone-root
 * depth-1 probe) is recomputed on every entry to the screen, so adding or
 * removing a clone root via the Settings dialog and reopening New session
 * sees the change. React Query still serves the cached payload first to
 * keep the screen responsive — the refetch happens in the background and
 * patches the list when it lands.
 */
export function useRepositoriesQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<RepositoriesResponse> {
  return useQuery({
    queryKey: queryKeys.repositories,
    queryFn: () => client.getRepositories(),
    enabled,
    staleTime: 0,
  });
}

/**
 * Open pull requests for the new-session PR tab
 * (`GET /api/prs?lens=…`), one query per lens so the reviewer and
 * author sections refresh independently. Gated by `enabled` so it only
 * fetches while the tab is mounted; `retry` is disabled because the
 * unauthenticated/uninstalled-gh case is reported in-band (200 +
 * `gh_available: false`), not as an error to retry.
 */
export function usePullRequestsQuery(
  client: ApiClient,
  lens: PullRequestLens,
  enabled: boolean,
): UseQueryResult<PullRequestsResponse> {
  return useQuery({
    queryKey: queryKeys.pullRequests(lens),
    queryFn: () => client.getPullRequests(lens),
    enabled,
    retry: false,
  });
}

/**
 * Per-provider launch availability (`GET /api/providers`) for the new-session
 * provider selector. Cached for the page lifetime — binary presence on the
 * server host only changes across a server restart, which ends the browser
 * session anyway. `retry` is off: unavailability is reported in-band
 * (`available: false`), not as an error to retry, and a failed fetch simply
 * leaves every provider enabled (fail-open, so the selector never wrongly locks
 * a user out).
 */
export function useProvidersQuery(
  client: ApiClient,
): UseQueryResult<ProvidersResponse> {
  return useQuery({
    queryKey: queryKeys.providers,
    queryFn: () => client.getProviders(),
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
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
 * Set a launch option's `default_enabled` flag (`PATCH
 * /api/launch-options/{id}`); refresh the list on success so the toggled state
 * is reflected.
 */
export function useUpdateLaunchOptionMutation(
  client: ApiClient,
): UseMutationResult<
  LaunchOption,
  Error,
  { id: number; body: UpdateLaunchOptionRequest }
> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, body }: { id: number; body: UpdateLaunchOptionRequest }) =>
      client.updateLaunchOption(id, body),
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

/**
 * The registered clone roots (`GET /api/clone-roots`), newest first. Used by
 * the Settings dialog's "Clone roots" section. Gated by `enabled` so it only
 * fetches while the section is mounted; mutations invalidate this key and the
 * Repository list to refresh both.
 *
 * Like {@link useRepositoriesQuery}, `staleTime: 0` keeps the New-session-
 * screen lifetime contract: every re-entry refetches so a registration made
 * elsewhere is reflected immediately.
 */
export function useCloneRootsQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<CloneRootsResponse> {
  return useQuery({
    queryKey: queryKeys.cloneRoots,
    queryFn: () => client.getCloneRoots(),
    enabled,
    staleTime: 0,
  });
}

/**
 * Register a clone root (`POST /api/clone-roots`). Invalidates the clone-root
 * list and the Repository list, since a new clone root may surface
 * previously-hidden clones in the Repository tab on the next render.
 */
export function useAddCloneRootMutation(
  client: ApiClient,
): UseMutationResult<CloneRoot, Error, CreateCloneRootRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateCloneRootRequest) => client.createCloneRoot(body),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.cloneRoots,
      });
      void queryClient.invalidateQueries({
        queryKey: queryKeys.repositories,
      });
    },
  });
}

/**
 * Request a repository clone (`POST /api/repositories/clone`).
 *
 * Deliberately invalidates nothing on success: the request only *starts* a job,
 * so nothing on the server has changed yet when it answers `202`. The
 * `repository_clone_completed` event is what flips `has_local_clone`, and the
 * event router refetches from there.
 *
 * Error presentation is the call site's job — a refused clone (an unregistered
 * root, an occupied destination) belongs inline on the row that asked for it, so
 * this hook touches no notification surface.
 */
export function useCloneRepositoryMutation(
  client: ApiClient,
): UseMutationResult<void, Error, CloneRepositoryRequest> {
  return useMutation({
    mutationFn: (body: CloneRepositoryRequest) => client.cloneRepository(body),
  });
}

/**
 * Unregister a clone root (`DELETE /api/clone-roots/{path_b64}`). Invalidates
 * the same two caches as the add mutation, since dropping a clone root can
 * drop clones from the Repository list.
 */
export function useRemoveCloneRootMutation(
  client: ApiClient,
): UseMutationResult<void, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => client.deleteCloneRoot(path),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.cloneRoots,
      });
      void queryClient.invalidateQueries({
        queryKey: queryKeys.repositories,
      });
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
 * Release a restored send into the normal queued flow
 * (`POST /api/sends/{id}/release`); refresh the session's open-send list so
 * the chip's restored label gives way to the row's next truthful state
 * (dispatched, or plain queued when the session is closed or busy).
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

/**
 * Launch an external tool (VS Code today) at a session's cwd
 * (`POST /api/open-cwd`). Success is `204` with no toast — the editor
 * opening is the feedback. Errors surface via {@link ApiError} carrying
 * one of the `open_cwd_*` codes so the caller can render the specific
 * message (missing binary, unknown path, generic failure).
 *
 * No cache invalidation: opening the editor does not change any server
 * state Delta tracks.
 */
export function useOpenCwdMutation(
  client: ApiClient,
): UseMutationResult<void, Error, OpenCwdRequest> {
  return useMutation({
    mutationFn: (body: OpenCwdRequest) => client.openCwd(body),
  });
}

/**
 * The Delta workspace version (`GET /api/version`) for the navigator footer.
 * Cached for the page lifetime — the running server's version can only change
 * across a full restart, which itself terminates the browser session, so a
 * reload is the only path that can invalidate this. Retries are off (a failure
 * here just hides the footer version; it must not cascade to a retry burst).
 */
export function useVersionQuery(
  client: ApiClient,
): UseQueryResult<VersionResponse> {
  return useQuery({
    queryKey: queryKeys.version,
    queryFn: () => client.getVersion(),
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
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
