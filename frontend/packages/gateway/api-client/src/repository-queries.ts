import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  CloneRepositoryRequest,
  CreateCloneRootRequest,
  PullRequestsResponse,
  RepositoriesResponse,
  CloneRoot,
  CloneRootsResponse,
} from '@delta/wire-gen';
import type { ApiClient, PullRequestLens } from './http';
import { queryKeys } from './query-keys';

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
