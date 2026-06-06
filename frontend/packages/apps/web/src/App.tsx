import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ApiProvider } from './data/apiContext';
import { WorkspaceScreen } from './features/workspace/WorkspaceScreen';

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // The live channel drives freshness via cache invalidation, so we do
        // not also poll. Retries are off because a 404 means "no session yet".
        retry: false,
        refetchOnWindowFocus: false,
      },
    },
  });
}

const queryClient = createQueryClient();

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ApiProvider>
        <div className="h-full bg-slate-50 text-slate-900">
          <WorkspaceScreen />
        </div>
      </ApiProvider>
    </QueryClientProvider>
  );
}
