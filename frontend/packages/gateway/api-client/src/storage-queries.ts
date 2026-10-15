import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  DeleteSnapshotRequest,
  EraseResponse,
  PrunePreviewResponse,
  PruneSessionsRequest,
  PruneSessionsResponse,
  RemoveWorktreeRequest,
  StorageResponse,
  StorageWorktreesResponse,
} from '@delta/wire-gen';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

/** How long a fetched storage inventory counts as fresh (see {@link useStorageQuery}). */
const STORAGE_STALE_TIME_MS = 30_000;

/**
 * Where the server keeps its files (`GET /api/storage`), for the Settings
 * Storage category. `enabled` gates the fetch to while that category is
 * shown. The paths are fixed for the server's lifetime but the sizes are read
 * on each request, so the answer goes stale after 30 s: reopening the category
 * later shows the database as it is now, without refetching on every tab
 * switch in between.
 */
export function useStorageQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<StorageResponse> {
  return useQuery({
    queryKey: queryKeys.storage,
    queryFn: () => client.getStorage(),
    enabled,
    staleTime: STORAGE_STALE_TIME_MS,
  });
}

/**
 * The directories under the worktree base (`GET /api/storage/worktrees`), for
 * the Storage category's Worktrees block and the Erase everything category.
 * `enabled` gates the fetch to while either is shown. Each answer runs `git`
 * once per directory, so it is
 * kept fresh as long as the storage inventory is; a removal invalidates it.
 */
export function useStorageWorktreesQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<StorageWorktreesResponse> {
  return useQuery({
    queryKey: queryKeys.storageWorktrees,
    queryFn: () => client.getStorageWorktrees(),
    enabled,
    staleTime: STORAGE_STALE_TIME_MS,
  });
}

/**
 * Remove one leftover worktree (`DELETE /api/storage/worktrees`). Invalidates
 * the worktree list, so the removed row goes — and, after a refusal, so the
 * row shows what the server found (a worktree that gained changes, or one a
 * session started in since).
 */
export function useRemoveStorageWorktreeMutation(
  client: ApiClient,
): UseMutationResult<void, Error, RemoveWorktreeRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: RemoveWorktreeRequest) => client.removeStorageWorktree(body),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.storageWorktrees });
    },
  });
}

/**
 * Delete one migration snapshot (`DELETE /api/storage/snapshots`).
 * Invalidates the storage inventory, which lists the snapshots.
 */
export function useDeleteSnapshotMutation(
  client: ApiClient,
): UseMutationResult<void, Error, DeleteSnapshotRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: DeleteSnapshotRequest) => client.deleteSnapshot(body),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.storage });
    },
  });
}

/**
 * The bulk session removal's preview (`GET /api/sessions/prune`): how many
 * closed sessions `criteria` would take now. `enabled` gates the fetch to
 * while the Storage category is shown. Never cached as fresh: the answer
 * changes as sessions close and age, and it is what the confirmation names.
 */
export function usePrunePreviewQuery(
  client: ApiClient,
  criteria: PruneSessionsRequest,
  enabled: boolean,
): UseQueryResult<PrunePreviewResponse> {
  const statuses = criteria.statuses ?? [];
  return useQuery({
    queryKey: queryKeys.prunePreview(criteria.older_than_days, statuses),
    queryFn: () => client.previewPruneSessions(criteria),
    enabled,
    staleTime: 0,
  });
}

/**
 * Remove old closed sessions in bulk (`POST /api/sessions/prune`).
 *
 * The server broadcasts `session_removed` for each one, which every tab
 * applies; the invalidations here refresh the acting tab's session list, the
 * database size in the storage inventory, the worktree list (a removed
 * session's clean worktree is gone, a dirty one is now a leftover) and every
 * preview, without waiting on the socket.
 */
export function usePruneSessionsMutation(
  client: ApiClient,
): UseMutationResult<PruneSessionsResponse, Error, PruneSessionsRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: PruneSessionsRequest) => client.pruneSessions(body),
    onSettled: () => {
      for (const queryKey of [
        queryKeys.sessions,
        queryKeys.storage,
        queryKeys.storageWorktrees,
        queryKeys.prunePreviewAll,
      ]) {
        void queryClient.invalidateQueries({ queryKey });
      }
    },
  });
}

/**
 * Erase everything this Delta created on the machine that holds no work
 * (`POST /api/storage/erase`). The server stops right after it answers, so a
 * success invalidates nothing: there is nothing left to refetch from, and
 * `onErased` replaces the app with the report. It is a mutation-level
 * callback, not one passed to `mutate`, so it still runs when the user has
 * left the Storage category while the erase was running.
 *
 * A failure can come after sessions were already closed and removed, so it
 * refreshes the same views a bulk removal does.
 */
export function useEraseEverythingMutation(
  client: ApiClient,
  onErased: (report: EraseResponse) => void,
): UseMutationResult<EraseResponse, Error, void> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => client.eraseEverything(),
    onSuccess: onErased,
    onError: () => {
      for (const queryKey of [
        queryKeys.sessions,
        queryKeys.storage,
        queryKeys.storageWorktrees,
        queryKeys.prunePreviewAll,
      ]) {
        void queryClient.invalidateQueries({ queryKey });
      }
    },
  });
}
