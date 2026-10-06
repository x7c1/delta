import type { ReactElement } from 'react';
import { render, type RenderResult } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../../data/apiContext';
import { ErasedGate } from './ErasedGate';

/**
 * Render a Storage block against the MSW mock backend at `http://localhost`,
 * with a fresh query cache and no retries, so a refused request shows at once,
 * inside the gate a successful erase replaces with its report.
 */
export function renderWithApi(ui: ReactElement): RenderResult {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <ErasedGate>{ui}</ErasedGate>
      </ApiProvider>
    </QueryClientProvider>,
  );
}
