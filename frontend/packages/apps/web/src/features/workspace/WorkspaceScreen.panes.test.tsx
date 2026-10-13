/**
 * The right pane: the lost-contact notice, and which of the terminal and the
 * comms log the workspace offers and mounts for each provider.
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
import { act, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import {
  SESSION_ID,
  SESSION_ID_2,
  SESSION_ID_4,
  MAIN_THREAD_ID,
  SESSION_2_MAIN_THREAD_ID,
  SESSION_4_MAIN_THREAD_ID,
  mockProviders,
} from '@delta/api-mocks';
import { useNavStore } from '../../store/navStore';
import {
  renderScreen,
  resetWorkspaceState,
  server,
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

  it('tells the user how to recover a re-adopted session whose hooks no longer arrive', async () => {
    // A session Delta re-adopted after a restart whose hook endpoint changed:
    // it is open, but its agent still calls the old hook URLs. The row says so,
    // and the transcript carries a one-line notice pointing at the terminal and
    // at Close-then-send.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID, {
      hooksUnreachable: true,
    });

    renderScreen();

    const notice = await screen.findByTestId('hooks-unreachable-notice');
    expect(notice).toHaveTextContent(
      "Delta lost contact with this session. Use the terminal, or choose Close in the session's menu and send again to resume.",
    );
    expect(screen.queryByTestId('readonly-notice')).not.toBeInTheDocument();
    // The composer refuses there and points at the notice instead.
    expect(
      screen.getByPlaceholderText(
        'Delta lost contact with this session — see the notice above…',
      ),
    ).toBeInTheDocument();
  });

  it('shows no lost-contact notice for an open session whose hooks arrive', async () => {
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(MAIN_THREAD_ID),
    );
    expect(await screen.findByTestId('terminal-toggle')).toBeInTheDocument();
    expect(
      screen.queryByTestId('hooks-unreachable-notice'),
    ).not.toBeInTheDocument();
  });

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

  it('shows the terminal pane for a Claude session when its terminal is open', async () => {
    // With the terminal open, the right pane mounts for a provider that has a
    // terminal — the gating must not strip it from Claude.
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: true },
    });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);

    renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toBeInTheDocument();
  });

  it('keeps the terminal pane mounted through a session whose terminal is closed', async () => {
    // The open state is per session, and the pane holds the bound sessions'
    // bridges. Unmounting it for B (terminal closed) would drop A's, and the
    // way back would rebuild A's xterm and run the pre-attach input wipe on it.
    terminalPaneMounts.count = 0;
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: true, [SESSION_ID_2]: false },
    });

    renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toBeInTheDocument();

    act(() => {
      useNavStore.setState({
        focusedSessionId: SESSION_ID_2,
        activeThreadId: SESSION_2_MAIN_THREAD_ID,
      });
    });
    expect(await screen.findByTestId('terminal-toggle')).toBeInTheDocument();
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();

    act(() => {
      useNavStore.setState({
        focusedSessionId: SESSION_ID,
        activeThreadId: MAIN_THREAD_ID,
      });
    });
    expect(await screen.findByTestId('terminal-pane')).toBeInTheDocument();
    expect(terminalPaneMounts.count).toBe(1);
  });

  it('leaves the terminal explaining itself for a starting session with no pane yet', async () => {
    // The row reads `spawning` on both sides of the pane coming up, so the flag
    // that separates "still preparing the launch" from "the agent is running in
    // a pane" is `pane_starting` — false here, and the terminal says so rather
    // than attaching to nothing.
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: true },
    });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID, {
      open: false,
      status: 'spawning',
    });

    renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toHaveAttribute(
      'data-pane-state',
      'preparing',
    );
  });

  it('offers a starting session its pane from the row alone, with no event', async () => {
    // The reload case, and the reason `pane_starting` is on the row at all: the
    // `spawn_pane_ready` announcement is a one-shot that this browser was not
    // listening for (it reloaded mid-launch, or is a second tab), and it is
    // never replayed. The first fetch of the list is all it gets — and a launch
    // stopped on an interactive prompt never binds, so if the terminal did not
    // attach from this, nothing would ever let the user answer it.
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: true },
    });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID, {
      open: false,
      status: 'spawning',
      paneStarting: true,
    });

    renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toHaveAttribute(
      'data-pane-state',
      'starting',
    );
  });

  it('gives a failed launch no pane to attach to', async () => {
    // A launch that never came up has no pane left, and saying it is closed
    // would offer a resume that cannot happen.
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: true },
    });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID, {
      open: false,
      status: 'failed',
    });

    renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toHaveAttribute(
      'data-pane-state',
      'failed',
    );
  });

  it('hides the terminal toggle and pane for a Codex session even with its terminal open', async () => {
    // A focused Codex session: the default `/api/providers` mock reports Codex
    // with no terminal. Even though its terminal is saved open (and would be
    // by default on the large layout), the capability gating must hide both the
    // toggle and the pane — a Codex session can never open a terminal.
    useNavStore.setState({
      focusedSessionId: SESSION_ID_4,
      terminalOpenBySession: { [SESSION_ID_4]: true },
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
    // The providers-loading window. The terminal is saved open, so the
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
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: true },
    });
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
    // The Codex leak this fix targets: with the terminal saved open for a
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
      terminalOpenBySession: { [SESSION_ID_4]: true },
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

  // --- Right-pane selection by capability ---------------------------------
  //
  // The workspace has two right-pane windows and picks between them from the
  // focused provider's capability profile — never from the provider name. The
  // rows below are the operation × state matrix that selection has to get right:
  // a terminal provider, a headless one (live and dormant), a pane-open flag left
  // behind by the OTHER provider, and the window before any capability is known.

  it('offers the comms toggle — not the terminal one — for a headless provider (Codex)', async () => {
    // The mirror image of the Claude row above: Codex reports no terminal but a
    // comms log, so the session gets the frame-log toggle instead. Neither is
    // chosen from the provider id.
    useNavStore.setState({ focusedSessionId: SESSION_ID_4 });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);

    renderScreen();

    expect(await screen.findByTestId('comms-toggle')).toBeInTheDocument();
    expect(screen.queryByTestId('terminal-toggle')).not.toBeInTheDocument();
  });

  it('offers neither the comms toggle nor the comms pane for a terminal provider (Claude), even with commsOpen persisted', async () => {
    // A provider with a terminal has no frame log to show, so offering the
    // toggle would hand the user a window that cannot exist — and a `commsOpen`
    // left behind by a Codex session must not open one either (the stale-flag
    // row, in this direction; the other direction is asserted below).
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: { [SESSION_ID]: false },
      commsOpen: true,
    });
    useSingleSessionOfProvider(SESSION_ID, 'claude', MAIN_THREAD_ID);

    renderScreen();

    expect(await screen.findByTestId('terminal-toggle')).toBeInTheDocument();
    expect(screen.queryByTestId('comms-toggle')).not.toBeInTheDocument();
    expect(screen.queryByTestId('comms-pane')).not.toBeInTheDocument();
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();
  });

  it('shows the comms pane for a live Codex session when commsOpen is set', async () => {
    useNavStore.setState({ focusedSessionId: SESSION_ID_4, commsOpen: true });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);

    renderScreen();

    const pane = await screen.findByTestId('comms-pane');
    // A live session is attachable, so the pane streams rather than idling.
    expect(pane.dataset.attachable).toBe('true');
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();
  });

  it('still shows the comms pane for a closed Codex session, marked not attachable', async () => {
    // A dormant session has no live adapter. The pane must still open — showing
    // its idle state (asserted in `CommsLogPane.test.tsx`) — rather than
    // vanishing, crashing, or spinning forever.
    useNavStore.setState({ focusedSessionId: SESSION_ID_4, commsOpen: true });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID, {
      open: false,
    });

    renderScreen();

    const pane = await screen.findByTestId('comms-pane');
    expect(pane.dataset.attachable).toBe('false');
  });

  it('does not open the comms pane from an open terminal', async () => {
    // The stale-persisted-state row. Each pane has its OWN flag precisely so a
    // open terminal cannot open the wrong window: with only the terminal open, a
    // focused Codex session shows NEITHER pane.
    useNavStore.setState({
      focusedSessionId: SESSION_ID_4,
      terminalOpenBySession: { [SESSION_ID_4]: true },
      commsOpen: false,
    });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);

    renderScreen();

    await waitFor(() =>
      expect(screen.getByTestId('comms-toggle')).toBeInTheDocument(),
    );
    expect(screen.queryByTestId('comms-pane')).not.toBeInTheDocument();
    expect(screen.queryByTestId('terminal-pane')).not.toBeInTheDocument();
  });

  it('withholds the comms pane and toggle until the providers query resolves', async () => {
    // Capability unknown → fail closed, with no historical default to preserve:
    // a browser must never open a `/comms` socket for a session whose provider
    // may not have one. Once Codex's profile arrives, both appear.
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
    useNavStore.setState({ focusedSessionId: SESSION_ID_4, commsOpen: true });
    useSingleSessionOfProvider(SESSION_ID_4, 'codex', SESSION_4_MAIN_THREAD_ID);

    renderScreen();

    // Fully rendered, providers still pending → nothing offered, nothing mounted.
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_4_MAIN_THREAD_ID,
      ),
    );
    expect(screen.queryByTestId('comms-pane')).not.toBeInTheDocument();
    expect(screen.queryByTestId('comms-toggle')).not.toBeInTheDocument();

    resolveProviders();
    expect(await screen.findByTestId('comms-pane')).toBeInTheDocument();
  });
});
