import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { delay, http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { WorkspaceScreen } from './WorkspaceScreen';

// The live event source opens a real WebSocket outside mock mode, and the
// terminal pane drives xterm.js — neither is meaningful in jsdom. Stub both so
// the test exercises only the bootstrap branch's structure.
vi.mock('../../data/useSessionEvents', () => ({
  useSessionEvents: () => {},
}));
vi.mock('../terminal/TerminalPane', () => ({
  TerminalPane: () => <div data-testid="terminal-pane" />,
}));

// jsdom does not implement matchMedia, which `useMediaQuery` relies on.
beforeAll(() => {
  vi.stubGlobal(
    'matchMedia',
    (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
});

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderScreen() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <WorkspaceScreen />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('WorkspaceScreen first-run bootstrap', () => {
  beforeEach(() => {
    useNavStore.setState({ activeThreadId: null, terminalOpen: true });
  });

  it('renders the terminal and a usable instruction when no session exists yet', async () => {
    // Fresh database: the session row is only created on the first hook, so
    // `GET /api/session` 404s while ensure-session still succeeds.
    server.use(
      http.post('*/api/session', () =>
        HttpResponse.json({ status: 'ready' }),
      ),
      http.get('*/api/session', () =>
        HttpResponse.json({ error: 'no session' }, { status: 404 }),
      ),
    );

    renderScreen();

    await waitFor(() =>
      expect(screen.getByText('Start the conversation')).toBeInTheDocument(),
    );
    // The embedded terminal is the only pre-session input channel.
    expect(screen.getByTestId('terminal-pane')).toBeInTheDocument();
    expect(
      screen.getByText(/Type your first message in the terminal below/),
    ).toBeInTheDocument();
    // The misleading dead-end copy with no input is gone.
    expect(
      screen.queryByText(/send your first message to Claude to begin/),
    ).not.toBeInTheDocument();
  });
});

describe('WorkspaceScreen active-thread fallback timing', () => {
  // The default `main_thread_id` (thread 1) the mocks report, and a thread id
  // that is absent from the listing — standing in for a freshly branched child
  // thread the navigator has switched to before the invalidated threads query
  // has refetched it.
  const MAIN_THREAD_ID = 1;
  const ABSENT_THREAD_ID = 999;

  beforeEach(() => {
    useNavStore.setState({
      activeThreadId: ABSENT_THREAD_ID,
      terminalOpen: false,
    });
  });

  it('does not revert to main while the threads query is still in flight', async () => {
    // Hold the threads response open so the query stays `isFetching` after the
    // component mounts. The active thread is absent from the (not-yet-arrived)
    // listing, but the existence-based fallback must NOT fire yet, mirroring a
    // branch send whose new child thread has not been refetched.
    // The listing contains main (thread 1) but never the absent active thread,
    // so the existence check (which requires a non-empty listing) can fire once
    // the query settles.
    server.use(
      http.get('*/api/threads', async () => {
        await delay(150);
        return HttpResponse.json({
          threads: [
            {
              id: MAIN_THREAD_ID,
              session_id: 'sess-mock-1',
              title: 'main',
              parent_thread_id: null,
              root_message_uuid: null,
              created_at: '2026-01-01T00:00:00Z',
            },
          ],
        });
      }),
    );

    renderScreen();

    // Wait until the normal workspace has mounted (session resolved).
    await waitFor(() =>
      expect(screen.queryByText('Connecting to the session…')).toBeNull(),
    );
    // While fetching, the absent active thread must be left untouched.
    expect(useNavStore.getState().activeThreadId).toBe(ABSENT_THREAD_ID);

    // Once the query settles with the thread still absent, the legitimate
    // fallback reconciles the stale active thread to main.
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(MAIN_THREAD_ID),
    );
  });
});
