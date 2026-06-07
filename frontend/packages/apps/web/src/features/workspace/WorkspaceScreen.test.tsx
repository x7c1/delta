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
import { http, HttpResponse } from 'msw';
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
