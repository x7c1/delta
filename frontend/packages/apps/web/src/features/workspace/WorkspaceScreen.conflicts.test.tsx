/**
 * Permission and question answers that conflict: the provider-specific
 * guidance, and the refetch that reveals a session closed behind the tab.
 *
 * Shared fixtures live in WorkspaceScreen.testkit.tsx.
 */
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
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import {
  SESSION_ID,
  SESSION_ID_4,
  MAIN_THREAD_ID,
  SESSION_4_MAIN_THREAD_ID,
} from '@delta/api-mocks';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import {
  renderScreen,
  resetWorkspaceState,
  server,
  singleSessionList,
  stubMatchMedia,
  useSingleSessionOfProvider,
} from './WorkspaceScreen.testkit';

// The live event source opens a real WebSocket outside mock mode, and the
// terminal pane drives xterm.js — neither is meaningful in jsdom. Stub both.
vi.mock('../../data/useSessionEvents', () => ({
  useSessionEvents: () => {},
}));

// Mounts of the stubbed terminal pane, so a test can tell a pane that was only
// hidden from one that was unmounted and rebuilt (which would drop every bridge
// it holds).
const { terminalPaneMounts } = vi.hoisted(() => ({
  terminalPaneMounts: { count: 0 },
}));
vi.mock('../terminal/TerminalPane', async () => {
  const { useEffect } = await vi.importActual<typeof import('react')>('react');
  return {
    // `paneState` is surfaced as an attribute because it is a decision this
    // screen makes — how far the focused session's pane has got — while what
    // the pane does with it belongs to `TerminalPane.test.tsx`. A hidden pane
    // renders nothing here, so "is the terminal shown" stays a presence check.
    TerminalPane: ({
      paneState,
      hidden,
    }: {
      paneState: string;
      hidden?: boolean;
    }) => {
      useEffect(() => {
        terminalPaneMounts.count += 1;
      }, []);
      return hidden ? null : (
        <div data-testid="terminal-pane" data-pane-state={paneState} />
      );
    },
  };
});
// The comms pane opens a `/comms` WebSocket in its effect, which is as
// meaningless in jsdom as the terminal's attach. Its own suite
// (`CommsLogPane.test.tsx`) covers what it renders per session state; here the
// question is only WHICH pane the workspace mounts.
vi.mock('../comms/CommsLogPane', () => ({
  CommsLogPane: ({ attachable }: { attachable: boolean }) => (
    <div data-testid="comms-pane" data-attachable={String(attachable)} />
  ),
}));

beforeAll(stubMatchMedia);

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('WorkspaceScreen multi-session', () => {
  beforeEach(resetWorkspaceState);

  // Seed a pending permission notice for a session, as the live event router
  // would, so the transcript pane's notice card renders with its Allow/Deny
  // buttons.
  function seedPermissionNotice(sessionId: string, requestId: number) {
    useLiveStore.setState({
      notices: {
        [sessionId]: [
          {
            kind: 'permission',
            requestId,
            toolName: 'Bash',
            toolInput: '{"command":"cargo test"}',
            dismissed: false,
            queued: [],
            pendingCount: 1,
          },
        ],
      },
    });
  }

  it('gives a Codex session terminal-free guidance when its decision conflicts', async () => {
    // The wiring test for the notice's conflict fallback: the workspace is what
    // resolves the focused session's `has_terminal` and hands it to the
    // transcript pane. A Codex session (no terminal) whose decision comes back
    // `409 permission_not_pending` — the agent connection died with the dialog
    // open, or another tab answered it — must not be told to answer in a
    // terminal it does not have.
    useNavStore.setState({ focusedSessionId: SESSION_ID_4 });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);
    seedPermissionNotice(SESSION_ID_4, 11);
    server.use(
      http.post('*/api/permissions/:id/decision', () =>
        HttpResponse.json(
          { error: 'not pending', code: 'permission_not_pending' },
          { status: 409 },
        ),
      ),
    );

    renderScreen();

    const notice = await screen.findByTestId('permission-notice');
    // Wait for the providers query to settle before deciding, so the assertion
    // is about the resolved capability and not the card's unknown-capability
    // default. The comms toggle is that query's visible effect for Codex.
    await screen.findByTestId('comms-toggle');
    fireEvent.click(within(notice).getByRole('button', { name: 'Allow' }));

    expect(
      await within(notice).findByTestId('permission-notice-unanswerable'),
    ).toBeInTheDocument();
    expect(
      within(notice).queryByText('Answer the prompt in the terminal.'),
    ).not.toBeInTheDocument();
    expect(
      within(notice).queryByRole('button', { name: 'Open terminal' }),
    ).not.toBeInTheDocument();
    expect(
      within(notice).getByRole('button', { name: 'Dismiss' }),
    ).toBeInTheDocument();
  });

  it('keeps the terminal guidance for a session whose capability never resolves', async () => {
    // The other half of the wiring, and the reason the notice gets the RAW
    // capability rather than the pane's fail-closed `focusedHasTerminal`: here
    // `GET /api/providers` fails and the query does not retry, so this page will
    // never learn that Claude has a terminal. The fail-closed flag reads `false`
    // in exactly that state — handing it to the card would answer a live TUI
    // prompt's only actionable instruction with "can no longer be answered". The
    // raw tri-state stays `undefined`, so HAS_TERMINAL_WHEN_UNKNOWN preserves
    // today's terminal guidance.
    server.use(
      http.get('*/api/providers', () =>
        HttpResponse.json({ error: 'providers unavailable' }, { status: 500 }),
      ),
      http.post('*/api/permissions/:id/decision', () =>
        HttpResponse.json(
          { error: 'not pending', code: 'permission_not_pending' },
          { status: 409 },
        ),
      ),
    );
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);
    seedPermissionNotice(SESSION_ID, 12);

    renderScreen();

    const notice = await screen.findByTestId('permission-notice');
    fireEvent.click(within(notice).getByRole('button', { name: 'Allow' }));

    expect(
      await within(notice).findByText('Answer the prompt in the terminal.'),
    ).toBeInTheDocument();
    expect(
      within(notice).getByRole('button', { name: 'Open terminal' }),
    ).toBeInTheDocument();
    expect(
      within(notice).queryByTestId('permission-notice-unanswerable'),
    ).not.toBeInTheDocument();
  });

  /**
   * Serve the session list as a tab that missed `session_closed` would meet it:
   * the session is listed open until `closeOnServer` runs, and closed on every
   * fetch after that. Counts the fetches, so a test can tell a refetch the 409
   * triggered from the closed copy merely being reachable.
   */
  function sessionClosedBehindTheTabsBack() {
    let listFetches = 0;
    let open = true;
    server.use(
      http.get('*/api/sessions', () => {
        listFetches += 1;
        return HttpResponse.json(
          singleSessionList(SESSION_ID, 'claude', MAIN_THREAD_ID, { open }),
        );
      }),
    );
    return {
      listFetches: () => listFetches,
      closeOnServer: () => {
        open = false;
      },
    };
  }

  it('refetches a session it believes open when a decision conflicts, and shows the closed copy once it is closed', async () => {
    // The tab's live socket went silently half-open: the server closed the
    // session, but this tab never heard `session_closed`, so it still lists the
    // session open. The 409 is the first sign; it must refetch the session
    // rather than point at a terminal that no longer exists.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    const list = sessionClosedBehindTheTabsBack();
    seedPermissionNotice(SESSION_ID, 13);
    server.use(
      http.post('*/api/permissions/:id/decision', () =>
        HttpResponse.json(
          { error: 'not pending', code: 'permission_not_pending' },
          { status: 409 },
        ),
      ),
    );

    renderScreen();

    const notice = await screen.findByTestId('permission-notice');
    // The tab believes the session open: no closed notice on the composer.
    expect(screen.queryByTestId('readonly-notice')).not.toBeInTheDocument();
    const fetchesBefore = list.listFetches();
    list.closeOnServer();
    fireEvent.click(within(notice).getByRole('button', { name: 'Allow' }));

    await waitFor(() =>
      expect(list.listFetches()).toBeGreaterThan(fetchesBefore),
    );
    expect(
      await within(notice).findByTestId('permission-notice-session-closed'),
    ).toBeInTheDocument();
    expect(
      within(notice).queryByText('Answer the prompt in the terminal.'),
    ).not.toBeInTheDocument();
    expect(
      within(notice).queryByRole('button', { name: 'Open terminal' }),
    ).not.toBeInTheDocument();
  });

  it('refetches a session it believes open when a question answer conflicts, and shows the closed copy once it is closed', async () => {
    // The question card's twin of the case above: `question_not_pending` on a
    // session the tab still lists open. Without the refetch the card would keep
    // sending the user to the terminal of a session that is gone.
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      activeThreadId: MAIN_THREAD_ID,
    });
    const list = sessionClosedBehindTheTabsBack();
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'question',
            requestId: 14,
            threadId: MAIN_THREAD_ID,
            toolInput: JSON.stringify({
              questions: [
                {
                  question: 'Which one?',
                  header: 'Pick',
                  options: [{ label: 'This' }, { label: 'That' }],
                  multiSelect: false,
                },
              ],
            }),
            dismissed: false,
          },
        ],
      },
    });
    server.use(
      http.post('*/api/sessions/:id/questions/:requestId/answer', () =>
        HttpResponse.json(
          { error: 'not pending', code: 'question_not_pending' },
          { status: 409 },
        ),
      ),
    );

    renderScreen();

    const card = await screen.findByTestId('question-card');
    const fetchesBefore = list.listFetches();
    list.closeOnServer();
    fireEvent.click(within(card).getByTestId('question-option-0-0'));

    await waitFor(() =>
      expect(list.listFetches()).toBeGreaterThan(fetchesBefore),
    );
    expect(
      await within(card).findByTestId('question-session-closed'),
    ).toBeInTheDocument();
    expect(within(card).queryByTestId('question-error')).not.toBeInTheDocument();
    expect(
      within(card).queryByRole('button', { name: 'Open terminal' }),
    ).not.toBeInTheDocument();
  });
});
