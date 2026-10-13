import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  LatestReleaseResponse,
  UpdateDownload,
  UpdateInstall,
  VersionResponse,
} from '@delta/wire-gen';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

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

/**
 * How often the navigator footer re-asks for a newer release (see
 * {@link useLatestReleaseQuery}): hourly. The server itself checks GitHub only
 * every 6 hours, so polling faster would only re-read the same answer.
 */
const LATEST_RELEASE_POLL_MS = 60 * 60 * 1000;

/**
 * How soon after its first answer the footer asks once more, before settling
 * into {@link LATEST_RELEASE_POLL_MS}. A page opened together with the server
 * — the desktop app's window always is — asks before the server's first check
 * has run (a few seconds after startup, plus up to the request's 10 s
 * timeout), so that first answer is always "nothing newer"; without this
 * second look the notice would wait up to an hour.
 */
const LATEST_RELEASE_FIRST_RECHECK_MS = 30 * 1000;

/**
 * How often the footer re-asks while the newer release downloads or
 * installs, to follow its progress and catch its outcome.
 */
const UPDATE_POLL_MS = 1000;

/**
 * A published release newer than the running server (`GET
 * /api/latest-release`), for the notice next to the version in the navigator
 * footer, with what the footer may offer beside it and how its download is
 * going. Asked again once {@link LATEST_RELEASE_FIRST_RECHECK_MS} after the
 * first answer, to catch the server's first check, then polled hourly so a
 * page left open for days learns about a release without a reload — and every
 * {@link UPDATE_POLL_MS} while a download or an install runs. The server
 * answers from memory, so each poll is cheap. Retries are off for the same reason as
 * {@link useVersionQuery}: a failure only hides the notice until the next
 * poll.
 */
export function useLatestReleaseQuery(
  client: ApiClient,
): UseQueryResult<LatestReleaseResponse> {
  return useQuery({
    queryKey: queryKeys.latestRelease,
    queryFn: () => client.getLatestRelease(),
    refetchInterval: (query) => {
      const latest = query.state.data;
      if (
        latest?.download?.state === 'downloading' ||
        latest?.install?.state === 'installing'
      ) {
        return UPDATE_POLL_MS;
      }
      return query.state.dataUpdateCount + query.state.errorUpdateCount < 2
        ? LATEST_RELEASE_FIRST_RECHECK_MS
        : LATEST_RELEASE_POLL_MS;
    },
    refetchIntervalInBackground: true,
    staleTime: LATEST_RELEASE_FIRST_RECHECK_MS,
    retry: false,
  });
}

/**
 * The footer's Update action (`POST /api/latest-release/download`): starts
 * the newer release's download, or joins the one running. The state it
 * answers is written into {@link useLatestReleaseQuery}'s cache at once, which
 * then polls every {@link UPDATE_POLL_MS} while it runs; a refusal is
 * the mutation's error, whose message the footer shows as the cause.
 */
export function useDownloadLatestReleaseMutation(
  client: ApiClient,
): UseMutationResult<UpdateDownload, Error, void> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => client.downloadLatestRelease(),
    onSuccess: (download) => {
      queryClient.setQueryData<LatestReleaseResponse>(
        queryKeys.latestRelease,
        (latest) => (latest ? { ...latest, download } : latest),
      );
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: queryKeys.latestRelease }),
  });
}

/**
 * The footer's Install action (`POST /api/latest-release/install`): starts
 * installing the ready download, or joins the install running. Like
 * {@link useDownloadLatestReleaseMutation}, the state it answers goes into
 * {@link useLatestReleaseQuery}'s cache at once, which then polls while the
 * install runs; a refusal is the mutation's error.
 */
export function useInstallLatestReleaseMutation(
  client: ApiClient,
): UseMutationResult<UpdateInstall, Error, void> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => client.installLatestRelease(),
    onSuccess: (install) => {
      queryClient.setQueryData<LatestReleaseResponse>(
        queryKeys.latestRelease,
        (latest) => (latest ? { ...latest, install } : latest),
      );
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: queryKeys.latestRelease }),
  });
}

/**
 * The footer's Restart action (`POST /api/latest-release/restart`): the
 * server answers and stops, and the desktop app starts again as the
 * installed version, so nothing is refetched after a success.
 */
export function useRestartLatestReleaseMutation(
  client: ApiClient,
): UseMutationResult<void, Error, void> {
  return useMutation({
    mutationFn: () => client.restartLatestRelease(),
  });
}
