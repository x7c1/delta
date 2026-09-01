import { createContext, useContext, type ReactNode } from 'react';
import { ApiClient } from '@delta/api-client';
import { apiBaseUrl, authToken } from '../config';

const ApiContext = createContext<ApiClient | null>(null);

export interface ApiProviderProps {
  client?: ApiClient;
  children: ReactNode;
}

/** Provides the singleton {@link ApiClient} (the only fetch holder) to the tree. */
export function ApiProvider({ client, children }: ApiProviderProps) {
  const value =
    client ?? new ApiClient({ baseUrl: apiBaseUrl(), token: authToken() });
  return <ApiContext.Provider value={value}>{children}</ApiContext.Provider>;
}

export function useApiClient(): ApiClient {
  const client = useContext(ApiContext);
  if (!client) {
    throw new Error('useApiClient must be used within an ApiProvider');
  }
  return client;
}
