/**
 * New-session spawns: focusing a tracked spawn, handing its focus over, and
 * the outcomes of a launch.
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
import { http, HttpResponse } from 'msw';
import { SESSION_ID, SESSION_ID_2, MAIN_THREAD_ID } from '@delta/api-mocks';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import {
  acceptNextSendAs,
  deliverEvent,
  listedThreads,
  renderScreen,
  renderScreenWithSender,
  resetWorkspaceState,
  SECOND_SPAWN_ID,
  SENDER_LABEL,
  server,
  stubMatchMedia,
  trackedSpawn,
  UNLISTED_SPAWN_ID,
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

  it('focuses a tracked spawn by its real id before its row is listed', async () => {
    // The new-session POST returned real ids and the spawn was tracked; the
    // user is still on the new-session screen. The workspace switches to that
    // exact id straight away — the server wrote the row before launching, so
    // there is nothing left to wait for — even though the id is nowhere in the
    // loaded session list (its refetch is still in flight). The tracked spawn
    // stays: it is what marks this focus as legitimately not-yet-listed, so the
    // list reconciliation must leave it standing rather than bouncing focus to
    // the head of the list.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({ spawns: [trackedSpawn(UNLISTED_SPAWN_ID)] });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID),
    );
    expect(useLiveStore.getState().spawns).toHaveLength(1);

    // Let the list load and the reconciliation run: the focus survives it.
    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBeGreaterThan(0),
    );
    expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID);

    // And the centre pane says the session is on its way. "Select a session"
    // here would read as if the user's Send had gone nowhere.
    expect(screen.getByText('Starting the session…')).toBeInTheDocument();
  });

  it('shows a tracked spawn’s terminal as starting, and none for a Codex launch, before its row is listed', async () => {
    // The beat between an accepted launch and its row reaching the loaded
    // pages. The provider is the launch's own, so a Codex launch gets no
    // terminal to flash up and vanish, and a Claude launch's terminal says it
    // is starting rather than asking for a session to be started.
    useNavStore.setState({
      focusedSessionId: UNLISTED_SPAWN_ID,
      terminalOpenBySession: { [UNLISTED_SPAWN_ID]: true },
    });
    useLiveStore.setState({ spawns: [trackedSpawn(UNLISTED_SPAWN_ID)] });

    const { unmount } = renderScreen();

    expect(await screen.findByTestId('terminal-pane')).toHaveAttribute(
      'data-pane-state',
      'preparing',
    );
    unmount();

    useLiveStore.setState({
      spawns: [{ ...trackedSpawn(UNLISTED_SPAWN_ID), provider: 'codex' }],
    });
    renderScreen();

    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBeGreaterThan(0),
    );
    expect(screen.getByText('Starting the session…')).toBeInTheDocument();
    expect(screen.queryByTestId('terminal-pane')).toBeNull();
    expect(screen.queryByTestId('terminal-toggle')).toBeNull();
  });

  it('forgets the terminal choice of a session no longer listed, but not a tracked spawn’s', async () => {
    // The prune runs against the complete list. A tracked spawn is not in it
    // yet (its refetch is still in flight), but a choice made on its screen in
    // that beat must survive; a session the list no longer has is dropped.
    // A single-page list, so the prune's complete-list gate opens on the first
    // response.
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
                pull_request_number: null,
              },
              open: true,
              main_thread_id: MAIN_THREAD_ID,
              threads: listedThreads(SESSION_ID, MAIN_THREAD_ID),
              last_activity_at: '2026-01-01T00:00:02Z',
            },
          ],
          next_cursor: null,
        }),
      ),
    );
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      terminalOpenBySession: {
        [SESSION_ID]: false,
        [UNLISTED_SPAWN_ID]: false,
        'sess-deleted-elsewhere': true,
      },
    });
    useLiveStore.setState({ spawns: [trackedSpawn(UNLISTED_SPAWN_ID)] });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().terminalOpenBySession).toEqual({
        [SESSION_ID]: false,
        [UNLISTED_SPAWN_ID]: false,
      }),
    );
  });

  it('hands a spawn’s focus over once, leaving New session usable', async () => {
    // The hand-over above is a one-shot. Once it has happened, pressing "New
    // session" must land on that screen and STAY there, rather than snapping
    // back to the still-spawning session. (Starting a second session from
    // there while the first is still coming up is covered below, through the
    // real submit path.)
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({ spawns: [trackedSpawn(UNLISTED_SPAWN_ID)] });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID),
    );
    // The hand-over is recorded on the spawn itself — that record, not which
    // screen is focused, is what says it has already happened.
    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({
        sessionId: UNLISTED_SPAWN_ID,
        status: 'spawning',
        focusHandedOver: true,
      }),
    ]);

    const newButton = await screen.findByRole('button', { name: 'New session' });
    fireEvent.click(newButton);

    // Flush the effects the focus change schedules: with a standing condition
    // this is exactly where focus bounced back to the spawn.
    await act(async () => {});
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    // Still spawning — the lock-out lasted precisely this window, so the spawn
    // has to be unresolved for the case to mean anything.
    expect(useLiveStore.getState().spawns).toHaveLength(1);
    expect(useLiveStore.getState().spawns[0].status).toBe('spawning');
  });

  it('spends the hand-over of every spawn awaiting one at the same moment', async () => {
    // Two new-session sends can travel at once — Send, then Send again while
    // the first POST is still out — and both can be tracked before the effect
    // runs, each owed a hand-over from this screen. Focus can only go to one,
    // the newest; the older one's hand-over is spent alongside it rather than
    // saved, so pressing "New session" afterwards opens that screen and STAYS
    // there instead of being yanked into the leftover spawn.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({
      spawns: [
        { ...trackedSpawn(UNLISTED_SPAWN_ID), focusHandedOver: false },
        { ...trackedSpawn(SECOND_SPAWN_ID), focusHandedOver: false },
      ],
    });

    renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SECOND_SPAWN_ID),
    );
    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({
        sessionId: UNLISTED_SPAWN_ID,
        status: 'spawning',
        focusHandedOver: true,
      }),
      expect.objectContaining({
        sessionId: SECOND_SPAWN_ID,
        status: 'spawning',
        focusHandedOver: true,
      }),
    ]);

    fireEvent.click(await screen.findByRole('button', { name: 'New session' }));

    // Flush the effects the focus change schedules — where the older spawn's
    // unspent hand-over would have fired.
    await act(async () => {});
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('carries the user into a session sent from the new-session screen', async () => {
    // The unchanged path, driven end to end through the real submit: Send from
    // the new-session screen, and the accepted POST hands focus to the session
    // it just started.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    acceptNextSendAs(UNLISTED_SPAWN_ID);

    renderScreenWithSender();

    fireEvent.click(await screen.findByRole('button', { name: SENDER_LABEL }));

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID),
    );
  });

  it('starts a second session from the new-session screen while the first spawns', async () => {
    // Back on the new-session screen with the first launch still coming up, a
    // second Send is handed focus exactly as the first was: the hand-over is
    // per spawn, and the one already spent neither suppresses the new one nor
    // stands in for it.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    acceptNextSendAs(UNLISTED_SPAWN_ID);

    renderScreenWithSender();

    const sender = await screen.findByRole('button', { name: SENDER_LABEL });
    fireEvent.click(sender);
    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID),
    );

    fireEvent.click(await screen.findByRole('button', { name: 'New session' }));
    acceptNextSendAs(SECOND_SPAWN_ID);
    fireEvent.click(sender);

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SECOND_SPAWN_ID),
    );
    // The first is still spawning, so the second hand-over really did happen
    // alongside a live one rather than after it resolved.
    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({
        sessionId: UNLISTED_SPAWN_ID,
        status: 'spawning',
        focusHandedOver: true,
      }),
      expect.objectContaining({
        sessionId: SECOND_SPAWN_ID,
        status: 'spawning',
        focusHandedOver: true,
      }),
    ]);
  });

  it('spends the hand-over of a send accepted while the user was elsewhere', async () => {
    // The regression: the user pressed Send, then clicked over to another
    // session while the POST was travelling. The spawn must not steal the
    // session they chose — and it must not save the hand-over up either. When
    // they later press "New session", that screen opens and STAYS open,
    // instead of dropping them into a session they had already moved on from.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    acceptNextSendAs(UNLISTED_SPAWN_ID);

    renderScreenWithSender();

    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBeGreaterThan(0),
    );
    fireEvent.click(await screen.findByRole('button', { name: SENDER_LABEL }));

    await waitFor(() => expect(useLiveStore.getState().spawns).toHaveLength(1));
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID);

    fireEvent.click(await screen.findByRole('button', { name: 'New session' }));

    // Flush the effects the focus change schedules — where a hand-over held
    // back for later would have fired.
    await act(async () => {});
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    // Still spawning, so the window in which a saved-up hand-over could fire
    // is genuinely open.
    expect(useLiveStore.getState().spawns[0].status).toBe('spawning');
  });

  it('leaves the new-session screen focused for a failed spawn', async () => {
    // A failed spawn's entry is kept for the launch configuration its session's
    // Retry reads, never as a hand-over candidate: opening the new-session
    // screen must not be undone by a launch that is already over. The card it
    // used to raise here is gone — the failure lives on its own session's
    // screen now — so this screen stays empty and ready for a fresh launch.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useLiveStore.setState({
      spawns: [
        { ...trackedSpawn(UNLISTED_SPAWN_ID), status: 'failed' as const },
      ],
    });

    renderScreen();

    const newButton = await screen.findByRole('button', { name: 'New session' });
    fireEvent.click(newButton);

    await act(async () => {});
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useLiveStore.getState().spawns).toHaveLength(1);
    expect(screen.queryByTestId('pending-item')).toBeNull();
    expect(screen.getByTestId('new-session-empty')).toBeInTheDocument();
  });

  it('releases the tracked spawn when the session registers', async () => {
    // The launch bound: `session_registered` ends the spawn window, so the
    // tracked entry is dropped there. Focus stays on the session, by then an
    // ordinary listed row.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({ spawns: [trackedSpawn(SESSION_ID_2)] });

    const { queryClient } = renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID_2),
    );

    deliverEvent(queryClient, {
      kind: 'session_registered',
      session_id: SESSION_ID_2,
    });

    await waitFor(() => expect(useLiveStore.getState().spawns).toHaveLength(0));
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID_2);
  });

  it('keeps focus on the focused session when its spawn fails', async () => {
    // The launch never bound. The focused session does NOT stop existing — its
    // row is kept and marked failed — and its screen is where the failure, the
    // prompt that never went out, and Retry / Remove now are. So focus stays
    // exactly where the user is, and the entry stays as what that screen's
    // Retry reads its launch configuration from.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({ spawns: [trackedSpawn(UNLISTED_SPAWN_ID)] });

    const { queryClient } = renderScreen();

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID),
    );

    deliverEvent(queryClient, {
      kind: 'spawn_failed',
      cancelled: false,
      session_id: UNLISTED_SPAWN_ID,
      pane_token: 'delta-9',
    });

    await waitFor(() =>
      expect(useLiveStore.getState().spawns).toEqual([
        expect.objectContaining({
          sessionId: UNLISTED_SPAWN_ID,
          status: 'failed',
        }),
      ]),
    );
    expect(useNavStore.getState().focusedSessionId).toBe(UNLISTED_SPAWN_ID);
  });

  it('does not steal focus for a spawn when the user moved on', async () => {
    // The effect's gate on its own: a spawn still owed a hand-over while focus
    // sits elsewhere. `useSubmitSend` spends such a hand-over as the POST
    // lands, so this state no longer arises in production — but the gate is
    // what keeps an unspent one from taking the session the user chose, and no
    // other case pins it.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    useLiveStore.setState({ spawns: [trackedSpawn(UNLISTED_SPAWN_ID)] });

    renderScreen();

    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBeGreaterThan(0),
    );
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID);
    expect(useLiveStore.getState().spawns).toHaveLength(1);
  });

  it('keeps the settings overlay open when a spawn takes focus', async () => {
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
    useLiveStore.setState({ spawns: [trackedSpawn(SESSION_ID_2)] });

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
});
