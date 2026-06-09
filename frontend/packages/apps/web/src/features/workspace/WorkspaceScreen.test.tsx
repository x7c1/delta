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
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import {
  SESSION_ID,
  SESSION_ID_2,
  SESSION_2_MAIN_THREAD_ID,
  createHandlers,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { WorkspaceScreen } from './WorkspaceScreen';

// The live event source opens a real WebSocket outside mock mode, and the
// terminal pane drives xterm.js — neither is meaningful in jsdom. Stub both.
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

describe('WorkspaceScreen multi-session', () => {
  beforeEach(() => {
    useNavStore.setState({
      focusedSessionId: null,
      activeThreadId: null,
      terminalOpen: false,
    });
  });

  it('shows the first page of sessions with a load-more sentinel when more remain', async () => {
    renderScreen();

    // The list is cursor-paginated (mock page size 2), so the first page holds
    // exactly the two most-recently-active sessions; the rest stay unloaded.
    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBe(2),
    );
    // The open one (sess-mock-1) is on page 1; its dot carries the "Open" name.
    expect(screen.getAllByRole('status', { name: 'Open' })).toHaveLength(1);
    // More pages remain, so the scroll sentinel is rendered to drive the next
    // fetch. (In jsdom the IntersectionObserver is inert, so no auto-load.)
    expect(
      screen.getByTestId('sessions-load-more-sentinel'),
    ).toBeInTheDocument();
  });

  it('focuses the most-recent open session on cold load', async () => {
    renderScreen();

    // sess-mock-1 is the only open session, so it is focused and its main
    // thread becomes active.
    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
  });

  it('falls back to the most-recently-active session when none are open', async () => {
    // The list arrives recency-descending (newest first), so with no open
    // session the head of the list is the most recent and gets focused.
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({
          sessions: [
            {
              session: {
                id: SESSION_ID_2,
                cwd: '/work',
                transcript_path: '/tmp/s2.jsonl',
                title: null,
                status: 'active',
                created_at: '2026-01-02T00:00:00Z',
              },
              open: false,
              main_thread_id: SESSION_2_MAIN_THREAD_ID,
              last_activity_at: '2026-01-02T00:00:02Z',
            },
            {
              session: {
                id: SESSION_ID,
                cwd: '/work',
                transcript_path: '/tmp/s1.jsonl',
                title: null,
                status: 'active',
                created_at: '2026-01-01T00:00:00Z',
              },
              open: false,
              main_thread_id: 1,
              last_activity_at: '2026-01-01T00:00:02Z',
            },
          ],
          next_cursor: null,
        }),
      ),
    );

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID_2),
    );
  });

  it('shows the new-session composer state when there are no sessions', async () => {
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({ sessions: [], next_cursor: null }),
      ),
    );

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS),
    );
    expect(screen.getByTestId('new-session-empty')).toBeInTheDocument();
  });

  it('flips the focused session to read-only after its Close button is clicked', async () => {
    renderScreen();

    // sess-mock-1 is open and auto-focused, so its transcript starts editable.
    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
    await waitFor(() =>
      expect(screen.getAllByRole('status', { name: 'Open' })).toHaveLength(1),
    );
    expect(screen.queryByTestId('readonly-notice')).not.toBeInTheDocument();

    // Every row carries a fixed-width actions menu, but only the open session's
    // is enabled; open it and pick the Close menu item. The closed session's
    // menu is disabled, so the enabled trigger is the one that opens.
    const actionTriggers = screen.getAllByRole('button', {
      name: /^Session actions for/,
    });
    const openTrigger = actionTriggers.find((button) => !button.hasAttribute('disabled'));
    expect(openTrigger).toBeDefined();
    fireEvent.click(openTrigger!);
    fireEvent.click(screen.getByRole('menuitem', { name: 'Close' }));

    // The mock flips the session closed; the refetched list shows no open dot
    // and the still-focused session re-renders read-only.
    await waitFor(() =>
      expect(screen.queryAllByRole('status', { name: 'Open' })).toHaveLength(0),
    );
    await waitFor(() =>
      expect(screen.getByTestId('readonly-notice')).toBeInTheDocument(),
    );
    // Focus stays on the now-closed session rather than snapping elsewhere.
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID);
  });

  it('renders a closed focused session read-only', async () => {
    useNavStore.setState({ focusedSessionId: SESSION_ID_2 });

    renderScreen();

    await waitFor(() =>
      expect(screen.getByTestId('readonly-notice')).toBeInTheDocument(),
    );
    // The closed session's main thread is reconciled as the active thread.
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_2_MAIN_THREAD_ID,
      ),
    );
  });
});
