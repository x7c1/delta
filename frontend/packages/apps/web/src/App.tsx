import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Button, ErrorBoundary } from '@delta/ui-kit';
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

/** Full-screen fallback for the app-wide error boundary. */
function AppCrash() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 bg-slate-50 text-sm text-slate-500">
      <p>Something went wrong.</p>
      <p className="text-xs text-slate-400">
        The app hit an unexpected error. Reload the page to recover.
      </p>
      <Button
        size="sm"
        variant="secondary"
        onClick={() => window.location.reload()}
      >
        Reload
      </Button>
    </div>
  );
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ApiProvider>
        {/* App-wide catch-all: a crash anywhere in the tree degrades to a
            recoverable notice instead of a blank page. Region-level boundaries
            (e.g. the terminal) handle their own failures before reaching here. */}
        <ErrorBoundary label="app" fallback={() => <AppCrash />}>
          <div className="h-full bg-slate-50 text-slate-900">
            <WorkspaceScreen />
          </div>
        </ErrorBoundary>
      </ApiProvider>
    </QueryClientProvider>
  );
}
