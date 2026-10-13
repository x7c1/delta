import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  CreateLaunchOptionRequest,
  LaunchOption,
  LaunchOptionsResponse,
  UpdateLaunchOptionRequest,
} from '@delta/wire-gen';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

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
