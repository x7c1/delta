import {
  useMutation,
  useQuery,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  GitBranchesResponse,
  GitRepoResponse,
  OpenCwdRequest,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/wire-gen';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

/**
 * One level of the working-directory browse (`GET /api/workdir/list`) for the
 * new-session picker. A `null` path lists the server default (`$HOME`);
 * `includeHidden` also lists dot-directories.
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
  includeHidden = false,
): UseQueryResult<WorkdirListResponse> {
  const queryKey = queryKeys.workdirList(path, includeHidden);
  return useQuery({
    queryKey,
    queryFn: () => client.getWorkdirList(path ?? undefined, includeHidden),
    enabled,
    retry: false,
    // Flipping `includeHidden` re-lists the same directory: keep showing the
    // listing already on screen until the other one arrives, instead of
    // swapping it for a loading state. A different path still loads fresh, so
    // a descend never shows the previous directory's rows.
    placeholderData: (previousData, previousQuery) =>
      previousQuery?.queryKey[1] === queryKey[1] ? previousData : undefined,
  });
}

/**
 * The user's home directory (the default `GET /api/workdir/list` target),
 * cached for path-abbreviation. Shares the `workdirList(null)` cache key with
 * the picker's initial browse (hidden directories off, the picker's default),
 * so it costs no extra request; `staleTime:
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
 * as soon as a directory is selected — the query is gated on the caller's
 * `enabled` and on `path !== null`, i.e. on a directory having been picked at
 * all. The picker only ever stores an absolute path or `null`, so the blank
 * `path` the endpoint rejects with a `400` never reaches it.
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
