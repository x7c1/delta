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
  SESSION_ID_4,
  MAIN_THREAD_ID,
  SESSION_2_MAIN_THREAD_ID,
  SESSION_2_BRANCH_THREAD_ID,
  SESSION_4_MAIN_THREAD_ID,
  createHandlers,
  mockProviders,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
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
      settingsOpen: false,
      terminalOpen: false,
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
    useLiveStore.setState({ spawns: [], unread: {} });
  });

  it('clears the focused session’s active thread unread on load', async () => {
    // A background turn finished on the focused session's main thread while the
    // user was elsewhere, so that thread carries unread. Focusing the session
    // activates its main thread, which clears that thread's unread (the row's
    // OR-aggregated dot clears with it).
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      activeThreadId: MAIN_THREAD_ID,
    });
    useLiveStore.setState({ unread: { [MAIN_THREAD_ID]: 1 } });

    renderScreen();

    await waitFor(() =>
      expect(useLiveStore.getState().unread[MAIN_THREAD_ID]).toBeUndefined(),
    );
  });

  it('leaves a non-active thread’s unread intact', async () => {
    // Only the active thread is cleared; a different thread (here a thread of
    // another session) that finished in the background keeps its unread until
    // the user activates it.
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      activeThreadId: MAIN_THREAD_ID,
    });
    useLiveStore.setState({
      unread: { [MAIN_THREAD_ID]: 1, [SESSION_2_MAIN_THREAD_ID]: 1 },
    });

    renderScreen();

    await waitFor(() =>
      expect(useLiveStore.getState().unread[MAIN_THREAD_ID]).toBeUndefined(),
    );
    expect(useLiveStore.getState().unread[SESSION_2_MAIN_THREAD_ID]).toBe(1);
  });

  it('focuses a tracked spawn by its real id once it appears in the list', async () => {
    // The new-session POST returned real ids and the spawn was tracked; the
    // user is still on the new-session screen. When the spawned session
    // registers (here: it is simply present in the list), the workspace must
    // focus exactly that id — not whatever happens to be at the head — and
    // stop tracking the spawn.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({
      spawns: [
        {
          sessionId: SESSION_ID_2,
          threadId: SESSION_2_MAIN_THREAD_ID,
          text: 'first message',
          workdir: null,
          status: 'spawning' as const,
        },
      ],
    });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID_2),
    );
    expect(useLiveStore.getState().spawns).toHaveLength(0);
  });

  it('does not steal focus for a registering spawn when the user moved on', async () => {
    // The spawn registers while the user is viewing another session: the
    // tracked spawn is released (its chip is over) but focus must stay put.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useLiveStore.setState({
      spawns: [
        {
          sessionId: SESSION_ID_2,
          threadId: SESSION_2_MAIN_THREAD_ID,
          text: 'first message',
          workdir: null,
          status: 'spawning' as const,
        },
      ],
    });

    renderScreen();

    await waitFor(() =>
      expect(useLiveStore.getState().spawns).toHaveLength(0),
    );
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID);
  });

  it('keeps the settings overlay open when a registering spawn takes focus', async () => {
    // The user sent the first message and opened Settings while the spawn was
    // still registering. The handover that follows is the WORKSPACE resolving
    // focus on its own — not the user navigating — so it must leave the modal
    // the user just opened alone. Dismissing it here made the Settings dialog
    // vanish milliseconds after it appeared, an order-dependent flake: only a
    // server that already holds sessions from earlier specs renders a session
    // card immediately, letting the click land before the handover.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      settingsOpen: true,
    });
    useLiveStore.setState({
      spawns: [
        {
          sessionId: SESSION_ID_2,
          threadId: SESSION_2_MAIN_THREAD_ID,
          text: 'first message',
          workdir: null,
          status: 'spawning' as const,
        },
      ],
    });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID_2),
    );
    expect(useNavStore.getState().settingsOpen).toBe(true);
  });

  it('keeps the settings overlay open when cold-start focus resolution runs', async () => {
    // Settings is persisted open across reloads, so the cold-start focus
    // reconciliation runs underneath an already-visible dialog. Resolving the
    // initial focus is not navigation either — the dialog stays.
    useNavStore.setState({ focusedSessionId: null, settingsOpen: true });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
    expect(useNavStore.getState().settingsOpen).toBe(true);
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
                branch_at_launch: null,
                repo_root: null,
                repository_display_name: null,
                provider: 'claude',
                provider_session_id: null,
                provider_thread_id: null,
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
                branch_at_launch: null,
                repo_root: null,
                repository_display_name: null,
                provider: 'claude',
                provider_session_id: null,
                provider_thread_id: null,
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

    // Phase B retired the auto-opened directory modal: the new-session
    // screen now leads with the inline 3-tab picker. The Directory tab
    // exposes the same Recent + Browse content, so first-run users still
    // have a clear path forward without a forced modal.
    expect(
      await screen.findByTestId('new-session-tabs'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('garbage-collects session-scoped localStorage keys for sessions that no longer exist', async () => {
    // Two preferences from earlier visits sit in localStorage: one for a
    // session that still exists (`SESSION_ID`) and one for an orphan
    // (`ghost-session`) that has been deleted since. The workspace's GC
    // hook should sweep the orphan after the session list finishes loading,
    // and leave the live session's preference untouched.
    //
    // The mock's single page covers the full list (next_cursor is null
    // unless there is more to fetch), so the GC's `!hasNextPage` gate
    // satisfies on the first response — no infinite-scroll plumbing
    // needed here.
    const liveKey = `delta.session.${SESSION_ID}.thread-timeline-overlay.expanded`;
    const orphanKey = 'delta.session.ghost-session.thread-timeline-overlay.expanded';
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({
          sessions: [
            {
              session: {
                id: SESSION_ID,
                cwd: '/work',
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
              },
              open: true,
              main_thread_id: MAIN_THREAD_ID,
              last_activity_at: '2026-01-01T00:00:02Z',
            },
          ],
          next_cursor: null,
        }),
      ),
    );
    window.localStorage.setItem(liveKey, 'true');
    window.localStorage.setItem(orphanKey, 'true');

    renderScreen();

    // The orphan key disappears once the session list is fully loaded; the
    // live session's key is left alone.
    await waitFor(() =>
      expect(window.localStorage.getItem(orphanKey)).toBeNull(),
    );
    expect(window.localStorage.getItem(liveKey)).toBe('true');
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

    // Every row carries a fixed-width actions menu and every trigger is enabled
    // (`Copy session ID` is always offered, even on a closed session). Pick the
    // open session's row by its untitled label — SESSION_ID has no title, so it
    // renders as `session <id-prefix>`, while the seeded closed sessions both
    // have titles. From that trigger, `Close` is only listed on an open session.
    const openTrigger = screen.getByRole('button', {
      name: /^Session actions for session /,
    });
    fireEvent.click(openTrigger);
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

  it('starts the new-session flow when "New session" is clicked from a real session', async () => {
    // Pin focus to a real session so the test does not depend on cold-load
    // auto-focus (the shared mock store can be mutated by earlier specs).
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    renderScreen();

    const newButton = await screen.findByRole('button', { name: 'New session' });
    fireEvent.click(newButton);

    // Focus moves to the sentinel and any prior workdir selection is reset.
    // Phase B retired the auto-opened modal — the click no longer pops the
    // dialog; the new-session screen leads with the inline tab picker.
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
    expect(useComposerStore.getState().workdirDialogOpen).toBe(false);
  });

  it('resets the workdir selection when "New session" is clicked while already in new-session', async () => {
    // Already in the new-session state with a stale selection — the
    // regression case where focus does not change. Clicking "New session"
    // must still wipe the stale workdir so the user starts from a clean
    // slate in the tab picker. The Repository tab may then immediately
    // auto-pick a default clone from the first registered repo; what
    // matters is that the stale value does not survive.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    useComposerStore.setState({
      workdirDialogOpen: false,
      newSessionWorkdir: '/stale/dir',
    });
    renderScreen();

    const newButton = await screen.findByRole('button', { name: 'New session' });
    fireEvent.click(newButton);

    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).not.toBe(
        '/stale/dir',
      );
    });
    // Phase B: no modal is auto-opened — the inline tab picker is the
    // primary entry point. The dialog stays closed.
    expect(useComposerStore.getState().workdirDialogOpen).toBe(false);
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

  it('identifies a session by its launch-time branch and repo basename', async () => {
    // The row's two-line header identifies a session by its launch context:
    // line 1 carries the *launch-time* local git branch (captured once on
    // spawn, never updated on resume), and line 2 left carries the basename of
    // the launch-time repository root with the time on the right. Both spans
    // are hover-titled with their full value; the per-span tooltips replace
    // the old card-wide cwd hover.
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

    // Line 1: the launch-time branch, with the full branch name on hover.
    const branch = await screen.findByTestId('session-branch');
    expect(branch).toHaveTextContent('feat/example');
    expect(branch.getAttribute('title')).toBe('feat/example');

    // Line 2 left: the repo basename, with the full repo_root path on hover.
    const repo = screen.getByTestId('session-repo');
    expect(repo).toHaveTextContent('delta');
    expect(repo.getAttribute('title')).toBe('/home/dev/projects/delta');

    // The last-activity time is still visible on line 2 (right-aligned).
    // Derive it the same way the component does so the assertion is
    // timezone-agnostic.
    const formattedTime = formatLocalDateTime(lastActivityAt);
    expect(formattedTime).not.toBeNull();
    expect(screen.getByText(formattedTime as string)).toBeInTheDocument();
  });

  // A single-session `/api/sessions` override for a given provider, so the
  // terminal-gating tests below do not depend on the shared mock store's mutated
  // state or on pagination. The focused session's threads still resolve through
  // the default handler (the store keeps every seed session's threads).
  function useSingleSessionOfProvider(
    id: string,
    provider: 'claude' | 'codex',
    mainThreadId: number,
  ) {
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({
          sessions: [
            {
              session: {
                id,
                cwd: '/work',
                transcript_path: '/tmp/s.jsonl',
                title: `${provider} session`,
                status: 'active',
                created_at: '2026-01-01T00:00:00Z',
                branch_at_launch: null,
                repo_root: null,
                repository_display_name: null,
                provider,
                provider_session_id: null,
                provider_thread_id: null,
              },
              open: true,
              main_thread_id: mainThreadId,
              last_activity_at: '2026-01-01T00:00:02Z',
            },
          ],
          next_cursor: null,
        }),
      ),
    );
  }

  it('shows the terminal toggle for a session whose provider has a terminal (Claude)', async () => {
    // A focused open Claude session; the default `/api/providers` mock reports
    // Claude with an attachable terminal. The workspace reads that capability
    // (never `provider === 'claude'`) and offers the terminal toggle.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(MAIN_THREAD_ID),
    );
    expect(await screen.findByTestId('terminal-toggle')).toBeInTheDocument();
  });

  it('shows the terminal pane for a Claude session when terminalOpen is set', async () => {
    // With the terminal open, the right pane mounts for a provider that has a
    // terminal — the gating must not strip it from Claude.
    useNavStore.setState({ focusedSessionId: SESSION_ID, terminalOpen: true });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);

    renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toBeInTheDocument();
  });

  it('hides the terminal toggle and pane for a Codex session even with terminalOpen persisted', async () => {
    // A focused Codex session: the default `/api/providers` mock reports Codex
    // with no terminal. Even though `terminalOpen` was persisted `true` (e.g.
    // from a previous Claude session), the capability gating must hide both the
    // toggle and the pane — a Codex session can never open a terminal.
    useNavStore.setState({
      focusedSessionId: SESSION_ID_4,
      terminalOpen: true,
    });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);

    renderScreen();

    // The transcript pane renders (its active thread reconciles), so the toggle
    // would appear here if the gating keyed off anything but the capability.
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_4_MAIN_THREAD_ID,
      ),
    );
    // Once the providers query settles (Codex → no terminal), the toggle and the
    // pane are both gone.
    await waitFor(() =>
      expect(screen.queryByTestId('terminal-toggle')).not.toBeInTheDocument(),
    );
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();
  });

  it('withholds the terminal pane for a Claude session until the providers query resolves', async () => {
    // The providers-loading window. `terminalOpen` is persisted `true`, so the
    // pane's mount hinges entirely on the capability gate. Until the profile is
    // known the gate must NOT fail open — otherwise the pane would mount and open
    // its `/pty` bridge before the capability resolves, which for a terminal-less
    // provider (Codex) fires the exact websocket the backend warns about. Here we
    // gate the `/api/providers` response on a manual promise and prove the pane
    // stays unmounted while the query is pending, then mounts once Claude's
    // terminal capability resolves — attaching only when the capability is known.
    let resolveProviders: () => void = () => {};
    const providersGate = new Promise<void>((resolve) => {
      resolveProviders = resolve;
    });
    server.use(
      http.get('*/api/providers', async () => {
        await providersGate;
        return HttpResponse.json({ providers: mockProviders() });
      }),
    );
    useNavStore.setState({ focusedSessionId: SESSION_ID, terminalOpen: true });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);

    renderScreen();

    // The session list resolves and its main thread reconciles, so the workspace
    // is fully rendered — but the providers query is still pending, so the pane
    // must be absent (the gate withholds it rather than failing open).
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(MAIN_THREAD_ID),
    );
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();

    // Once the capability is known (Claude → has terminal), the pane mounts and
    // attaches — the historical behaviour, just deferred to when it is safe.
    resolveProviders();
    expect(await screen.findByTestId('terminal-pane')).toBeInTheDocument();
  });

  it('never mounts the terminal pane for a Codex session across the providers-loading window', async () => {
    // The Codex leak this fix targets: with `terminalOpen` persisted `true` and a
    // Codex session focused on reload, the pane must never mount — not during the
    // loading window (gate withholds) and not after (Codex reports no terminal).
    // Since the pane is what opens the `/pty` bridge, a never-mounted pane is a
    // never-requested websocket.
    let resolveProviders: () => void = () => {};
    const providersGate = new Promise<void>((resolve) => {
      resolveProviders = resolve;
    });
    server.use(
      http.get('*/api/providers', async () => {
        await providersGate;
        return HttpResponse.json({ providers: mockProviders() });
      }),
    );
    useNavStore.setState({
      focusedSessionId: SESSION_ID_4,
      terminalOpen: true,
    });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);

    renderScreen();

    // Workspace fully rendered, providers still pending → pane withheld.
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_4_MAIN_THREAD_ID,
      ),
    );
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();

    // Providers resolve (Codex → no terminal): the pane stays absent.
    resolveProviders();
    await waitFor(() =>
      expect(screen.queryByTestId('terminal-toggle')).not.toBeInTheDocument(),
    );
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();
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
              },
              open: true,
              main_thread_id: 1,
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
