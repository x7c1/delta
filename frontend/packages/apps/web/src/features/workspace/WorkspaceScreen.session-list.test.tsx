/**
 * How a session is identified in the list, and what the workspace does when
 * the session list fails to load.
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
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { QueryClient } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { SESSION_ID, MAIN_THREAD_ID } from '@delta/api-mocks';
import { queryKeys } from '@delta/api-client';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useNotificationStore } from '../../store/notificationStore';
import {
  listedThreads,
  renderScreen,
  resetWorkspaceState,
  server,
  stubMatchMedia,
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

  it('identifies a session by its launch-time branch and repo basename', async () => {
    // The row's two-line header identifies a session by its launch context:
    // line 1 carries the *launch-time* local git branch (captured once on
    // spawn, never updated on resume), and line 2 left carries the basename of
    // the launch-time repository root, with the originating pull request in the
    // slot on the right. Both spans are hover-titled with their full value; the
    // per-span tooltips replace the old card-wide cwd hover.
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
                branch_at_launch: 'feat/example',
                repo_root: '/home/dev/projects/delta',
                repository_display_name: null,
                provider: 'claude',
                provider_session_id: null,
                provider_thread_id: null,
                pull_request_number: null,
              },
              open: true,
              main_thread_id: 1,
              threads: listedThreads(SESSION_ID, 1),
              last_activity_at: lastActivityAt,
            },
          ],
          next_cursor: null,
        }),
      ),
    );

    renderScreen();

    // Line 1: the launch-time branch, with the full branch name on hover.
    const branch = await screen.findByTestId('session-branch');
    expect(branch).toHaveTextContent('feat/example');
    expect(branch.getAttribute('title')).toBe('feat/example');

    // Line 2 left: the repo basename, with the full repo_root path on hover.
    const repo = screen.getByTestId('session-repo');
    expect(repo).toHaveTextContent('delta');
    expect(repo.getAttribute('title')).toBe('/home/dev/projects/delta');

    // Line 2 right: this session was not started from a PR, so the slot that
    // once held the last-activity time is empty. The list is ordered by
    // recency, so the timestamp said nothing the ordering did not.
    expect(
      screen.queryByTestId('session-pull-request'),
    ).not.toBeInTheDocument();
  });

  it("falls back to the cwd basename when a session has no launch repo_root", async () => {
    // A session launched outside any git repo (or one that predates the
    // spawn-time snapshot — older databases store NULL on both) still
    // identifies its working directory: line 2 falls back to the cwd's
    // basename, hover-titled with the full cwd path.
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({
          sessions: [
            {
              session: {
                id: SESSION_ID,
                cwd: '/home/dev/scratch',
                transcript_path: '/tmp/s1.jsonl',
                title: null,
                status: 'active',
                created_at: '2026-01-01T00:00:00Z',
                branch_at_launch: null,
                repo_root: null,
                repository_display_name: null,
                provider: 'claude',
                provider_session_id: null,
                provider_thread_id: null,
                pull_request_number: null,
              },
              open: true,
              main_thread_id: 1,
              threads: listedThreads(SESSION_ID, 1),
              last_activity_at: '2026-01-01T00:00:02Z',
            },
          ],
          next_cursor: null,
        }),
      ),
    );

    renderScreen();

    // Line 2: the cwd basename, with the full cwd path on hover.
    const repo = await screen.findByTestId('session-repo');
    expect(repo).toHaveTextContent('scratch');
    expect(repo.getAttribute('title')).toBe('/home/dev/scratch');

    // Line 1 falls back to the session label when no branch was recorded.
    const branch = screen.getByTestId('session-branch');
    expect(branch).toHaveTextContent(`session ${SESSION_ID.slice(0, 8)}`);
  });
});

describe('WorkspaceScreen session list failures', () => {
  /**
   * Whether `GET /api/sessions` currently fails. Flipped by each case to turn
   * the server off and on under a mounted workspace; when false the handler
   * returns nothing and the request falls through to the default mock.
   */
  let sessionsFailing = false;

  beforeEach(() => {
    sessionsFailing = false;
    server.use(
      http.get('*/api/sessions', () =>
        sessionsFailing
          ? HttpResponse.json({ error: 'unavailable' }, { status: 503 })
          : undefined,
      ),
    );
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      activeThreadId: null,
      preNewSessionFocus: null,
      settingsOpen: false,
      // The matchMedia stub reports the small layout, where a session's
      // terminal is closed unless saved open — so save it open.
      terminalOpenBySession: { [SESSION_ID]: true },
      terminalOpenWithoutSession: false,
      commsOpen: false,
    });
    useLiveStore.setState({ spawns: [], unread: {}, notices: {} });
    useNotificationStore.setState({ notifications: [] });
  });

  /** Run one background refetch of the session list to completion. */
  async function refetchSessions(queryClient: QueryClient) {
    await act(async () => {
      await queryClient.refetchQueries({ queryKey: queryKeys.sessions });
    });
  }

  function errorNotifications() {
    return useNotificationStore
      .getState()
      .notifications.filter((n) => n.tone === 'error');
  }

  it('shows the full-screen error when the first load fails, and Retry refetches', async () => {
    sessionsFailing = true;

    renderScreen();

    expect(
      await screen.findByText('Could not load sessions.'),
    ).toBeInTheDocument();
    // Nothing was ever on screen, so there is no list to keep and no snackbar
    // to raise on top of the full-screen message.
    expect(errorNotifications()).toHaveLength(0);

    sessionsFailing = false;
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(await screen.findAllByTestId('session-card')).not.toHaveLength(0);
    expect(screen.queryByText('Could not load sessions.')).toBeNull();
  });

  it('keeps the list and the focused session mounted across a failing refetch, notifying once per episode', async () => {
    const { queryClient } = renderScreen();

    const terminal = await screen.findByTestId('terminal-pane');
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(MAIN_THREAD_ID),
    );
    const cardCount = screen.getAllByTestId('session-card').length;

    // The server goes away under a loaded workspace.
    sessionsFailing = true;
    await refetchSessions(queryClient);

    await waitFor(() => expect(errorNotifications()).toHaveLength(1));
    expect(errorNotifications()[0]?.title).toBe(
      'Could not refresh the session list',
    );
    expect(screen.queryByText('Could not load sessions.')).toBeNull();
    expect(screen.getAllByTestId('session-card')).toHaveLength(cardCount);
    // The very same terminal element: not merely re-rendered, never unmounted.
    expect(screen.getByTestId('terminal-pane')).toBe(terminal);
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID);

    // Still down: the same failure episode raises no second notice.
    await refetchSessions(queryClient);
    expect(errorNotifications()).toHaveLength(1);
    expect(screen.getByTestId('terminal-pane')).toBe(terminal);

    // The server comes back, then fails again: a new episode, a new notice.
    sessionsFailing = false;
    await refetchSessions(queryClient);
    expect(errorNotifications()).toHaveLength(1);

    sessionsFailing = true;
    await refetchSessions(queryClient);
    await waitFor(() => expect(errorNotifications()).toHaveLength(2));
    expect(screen.getByTestId('terminal-pane')).toBe(terminal);
  });
});
