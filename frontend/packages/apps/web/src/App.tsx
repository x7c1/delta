import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Button, ErrorBoundary } from '@delta/ui-kit';
import { ApiProvider } from './data/apiContext';
import { NotificationSnackbar } from './features/notifications/NotificationSnackbar';
import { WorkspaceScreen } from './features/workspace/WorkspaceScreen';
import { ThemeProvider } from './hooks/themeContext';
import { VisualEffectsProvider } from './hooks/visualEffectsContext';

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
    <div className="flex h-full flex-col items-center justify-center gap-2 bg-surface-elevated text-secondary text-fg-muted">
      <p>Something went wrong.</p>
      <p className="text-caption text-fg-subtle">
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
        {/* ThemeProvider owns the singleton subscription to the active theme:
            it drives <html data-theme="...">, the matchMedia listener for the
            'system' preference, and the localStorage write — so any consumer
            (settings picker, xterm bridge) reads the same shared state. */}
        <ThemeProvider>
          {/* Mirrors ThemeProvider: derives the effective rich/flat look from
              the persisted visual-effects setting plus the environment and
              stamps <html data-effects="…"> so the decorative CSS gates. */}
          <VisualEffectsProvider>
            {/* App-wide catch-all: a crash anywhere in the tree degrades to a
                recoverable notice instead of a blank page. Region-level boundaries
                (e.g. the terminal) handle their own failures before reaching here. */}
            <ErrorBoundary label="app" fallback={() => <AppCrash />}>
              <div className="h-full bg-surface-elevated text-fg">
                <WorkspaceScreen />
                {/* App-wide snackbar, for failures and for outcomes the user
                    asked for alike. Rendered as a fixed overlay
                    outside the workspace layout so a bottom-anchored
                    notification never affects transcript scrolling. */}
                <NotificationSnackbar />
              </div>
            </ErrorBoundary>
          </VisualEffectsProvider>
        </ThemeProvider>
      </ApiProvider>
    </QueryClientProvider>
  );
}
