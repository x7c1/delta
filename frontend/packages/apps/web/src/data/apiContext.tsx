import { createContext, useContext, useMemo, type ReactNode } from 'react';
import { ApiClient } from '@delta/api-client';
import { apiBaseUrl, authToken } from '../config';

const ApiContext = createContext<ApiClient | null>(null);

export interface ApiProviderProps {
  client?: ApiClient;
  /** Called when the server answers `401`; see `ApiClientOptions`. */
  onUnauthorized?: () => void;
  children: ReactNode;
}

/** Provides the singleton {@link ApiClient} (the only fetch holder) to the tree. */
export function ApiProvider({
  client,
  onUnauthorized,
  children,
}: ApiProviderProps) {
  const value = useMemo(
    () =>
      client ??
      new ApiClient({
        baseUrl: apiBaseUrl(),
        token: authToken(),
        onUnauthorized,
      }),
    [client, onUnauthorized],
  );
  return <ApiContext.Provider value={value}>{children}</ApiContext.Provider>;
}

export function useApiClient(): ApiClient {
  const client = useContext(ApiContext);
  if (!client) {
    throw new Error('useApiClient must be used within an ApiProvider');
  }
  return client;
}
