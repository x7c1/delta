import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import type { ProvidersResponse } from '@delta/wire-gen';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

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
