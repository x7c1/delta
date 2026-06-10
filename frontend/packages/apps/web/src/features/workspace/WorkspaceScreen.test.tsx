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
  SESSION_2_BRANCH_THREAD_ID,
  createHandlers,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
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
      preNewSessionFocus: null,
      terminalOpen: false,
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
  });

  it('shows the first page of sessions when more remain', async () => {
    renderScreen();

    // The list is cursor-paginated (mock page size 2), so the first page holds
    // exactly the two most-recently-active sessions; the rest stay unloaded.
    // (The list is windowed, but two rows are well within the rendered window,
    // so both are mounted.)
    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBe(2),
    );
    // The open one (sess-mock-1) is on page 1; its dot carries the "Open" name.
    expect(screen.getAllByRole('status', { name: 'Open' })).toHaveLength(1);
    // More pages remain. In jsdom the scroll viewport has no measured height, so
    // the virtualizer cannot auto-advance its range; pagination is driven
    // explicitly here and exercised end-to-end by the Playwright specs.
  });

  it('focuses the most-recent open session on cold load', async () => {
    renderScreen();

    // sess-mock-1 is the only open session, so it is focused and its main
    // thread becomes active.
    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
  });

  it("expands a non-focused session's sub-thread tree without a click", async () => {
    renderScreen();

    // sess-mock-1 (open) is auto-focused; sess-mock-2 (closed, "scratch notes")
    // is visible but NOT focused. Each visible row fetches its own thread tree,
    // so the non-focused session shows its sub-thread ("scratch ideas") expanded
    // from the start — no click required. (Runs before the Close test below,
    // which permanently closes sess-mock-1 in the shared mock store.)
    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
    await waitFor(() =>
      expect(screen.getByText('scratch ideas')).toBeInTheDocument(),
    );

    // Clicking that sub-thread focuses its owning (previously non-focused)
    // session and makes the sub-thread active, so the center pane switches to it.
    fireEvent.click(screen.getByText('scratch ideas'));
    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID_2),
    );
    expect(useNavStore.getState().activeThreadId).toBe(
      SESSION_2_BRANCH_THREAD_ID,
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

    // First run (zero sessions): the directory picker is mandatory, so it opens
    // with no Cancel button — the user must choose a directory to proceed.
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(screen.queryByTestId('workdir-cancel')).not.toBeInTheDocument();
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

  it('starts the new-session flow when "New" is clicked from a real session', async () => {
    // Pin focus to a real session so the test does not depend on cold-load
    // auto-focus (the shared mock store can be mutated by earlier specs).
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    renderScreen();

    const newButton = await screen.findByRole('button', { name: 'New' });
    fireEvent.click(newButton);

    // Focus moves to the sentinel, any prior selection is reset, and the picker
    // is opened — all three driven by the single "New" click.
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
    expect(useComposerStore.getState().workdirDialogOpen).toBe(true);
  });

  it('re-opens the picker when "New" is clicked while already in new-session', async () => {
    // Already in the new-session state with a stale selection and the picker
    // dismissed — the regression case where focus does not change, so a
    // focus-driven auto-open would not re-fire.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    useComposerStore.setState({
      workdirDialogOpen: false,
      newSessionWorkdir: '/stale/dir',
    });
    renderScreen();

    const newButton = await screen.findByRole('button', { name: 'New' });
    fireEvent.click(newButton);

    // Clicking "New" again (still in new-session) must reset the selection and
    // re-open the picker.
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
    expect(useComposerStore.getState().workdirDialogOpen).toBe(true);
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

  it("shows the working-directory tail in a session's row", async () => {
    // The row carries the session's cwd compactly (its last two segments) so a
    // session is identifiable by where it runs. The full path and the
    // last-activity time stay available on hover via `title` — the time is no
    // longer rendered as visible row text.
    const lastActivityAt = '2026-01-01T00:00:02Z';
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({
          sessions: [
            {
              session: {
                id: SESSION_ID,
                cwd: '/home/dev/projects/delta',
                transcript_path: '/tmp/s1.jsonl',
                title: null,
                status: 'active',
                created_at: '2026-01-01T00:00:00Z',
              },
              open: true,
              main_thread_id: 1,
              last_activity_at: lastActivityAt,
            },
          ],
          next_cursor: null,
        }),
      ),
    );

    renderScreen();

    const tail = await screen.findByText('projects/delta');
    // The tooltip carries the full path on the first line and the formatted
    // last-activity time on the second (native `title` renders `\n` as a line
    // break). Derive the expected time the same way the component does so the
    // assertion is timezone-agnostic.
    const formattedTime = formatLocalDateTime(lastActivityAt);
    expect(formattedTime).not.toBeNull();
    const title = tail.getAttribute('title');
    expect(title).toBe(`/home/dev/projects/delta\n${formattedTime}`);
    expect(title).toContain('/home/dev/projects/delta');

    // The timestamp is not visible row text — only in the tooltip.
    expect(screen.queryByText(formattedTime as string)).not.toBeInTheDocument();
  });
});
