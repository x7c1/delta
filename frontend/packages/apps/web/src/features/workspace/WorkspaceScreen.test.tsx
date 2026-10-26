/**
 * Unread badges, cold-load focus, and the session lifecycle actions (New,
 * Close) in the multi-session workspace.
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
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import {
  SESSION_ID,
  SESSION_ID_2,
  MAIN_THREAD_ID,
  SESSION_2_MAIN_THREAD_ID,
  SESSION_2_BRANCH_THREAD_ID,
  mockThreads,
} from '@delta/api-mocks';
import { useCreateSendMutation } from '@delta/api-client';
import type { Thread } from '@delta/wire-gen';
import { useApiClient } from '../../data/apiContext';
import { applySessionEvent } from '../../data/applySessionEvent';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import {
  deliverEvent,
  listedThreads,
  renderScreen,
  resetWorkspaceState,
  server,
  stubMatchMedia,
  threadRow,
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

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('WorkspaceScreen multi-session', () => {
  beforeEach(resetWorkspaceState);

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

  it('never reveals a badge for events that landed while the thread was on screen', async () => {
    // The live-dogfooding regression, end to end. Thread A ("scratch ideas")
    // is the thread the user is reading while input typed straight into its
    // pane keeps arriving as `external_input`. Switching to another thread used
    // to REVEAL a "1" on A for exactly those events: the count was invisible
    // while A was active (its badge is suppressed) and no activation edge ever
    // came back to clear it, so it sat pinned to the thread the user had just
    // been reading.
    useSingleSessionOfProvider(SESSION_ID_2, 'claude', SESSION_2_MAIN_THREAD_ID);
    useNavStore.setState({
      focusedSessionId: SESSION_ID_2,
      activeThreadId: SESSION_2_BRANCH_THREAD_ID,
    });

    const { queryClient } = renderScreen();

    await waitFor(() => expect(threadRow('scratch ideas')).toBeInTheDocument());
    expect(useNavStore.getState().activeThreadId).toBe(
      SESSION_2_BRANCH_THREAD_ID,
    );

    for (const prompt of ['notification one', 'notification two']) {
      deliverEvent(queryClient, {
        kind: 'external_input',
        session_id: SESSION_ID_2,
        prompt,
      });
    }

    // The user moves to thread B — the session's main thread, reached by
    // clicking the card header. This is the transition that used to expose the
    // phantom count.
    fireEvent.click(screen.getByTestId('session-node'));
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_2_MAIN_THREAD_ID,
      ),
    );

    expect(within(threadRow('scratch ideas')).queryByText('1')).toBeNull();
    expect(within(threadRow('scratch ideas')).queryByText('2')).toBeNull();
    expect(
      useLiveStore.getState().unread[SESSION_2_BRANCH_THREAD_ID],
    ).toBeUndefined();
  });

  it('badges a thread whose turns complete after the user moved to another thread', async () => {
    // The other half of the invariant: suppressing the on-screen bump must not
    // mute genuine unread. Here thread B (main) is active from the start, so
    // completions on thread A ("scratch ideas") land while A is not the active
    // thread — they badge it, accumulate past 1, and clear for good once A is
    // selected.
    useSingleSessionOfProvider(SESSION_ID_2, 'claude', SESSION_2_MAIN_THREAD_ID);
    useNavStore.setState({
      focusedSessionId: SESSION_ID_2,
      activeThreadId: SESSION_2_MAIN_THREAD_ID,
    });

    const { queryClient } = renderScreen();

    await waitFor(() => expect(threadRow('scratch ideas')).toBeInTheDocument());

    const completeBranchTurn = () =>
      deliverEvent(queryClient, {
        kind: 'turn_completed',
        session_id: SESSION_ID_2,
        thread_id: SESSION_2_BRANCH_THREAD_ID,
        stop_reason: null,
      });

    completeBranchTurn();
    await waitFor(() =>
      expect(
        within(threadRow('scratch ideas')).getByText('1'),
      ).toBeInTheDocument(),
    );

    // Counts keep accumulating while the user is away — the badge is not a
    // boolean pinned at 1.
    completeBranchTurn();
    await waitFor(() =>
      expect(
        within(threadRow('scratch ideas')).getByText('2'),
      ).toBeInTheDocument(),
    );

    // Selecting the thread clears it, and switching away again with no new
    // events must not bring it back (the regression the user hit).
    fireEvent.click(threadRow('scratch ideas'));
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_2_BRANCH_THREAD_ID,
      ),
    );
    fireEvent.click(screen.getByTestId('session-node'));
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_2_MAIN_THREAD_ID,
      ),
    );

    expect(within(threadRow('scratch ideas')).queryByText('2')).toBeNull();
    expect(
      useLiveStore.getState().unread[SESSION_2_BRANCH_THREAD_ID],
    ).toBeUndefined();
  });

  it('drops a count that reached the on-screen thread despite the router guard', async () => {
    // The deactivation-edge backstop on its own. The router refuses to bump the
    // focused active thread, but it decides that from focus state that can lag
    // the screen (the windows are listed on the unread effect in
    // `WorkspaceScreen`). Routing a completion for thread A through such a
    // window — the router sees no active thread while A is in fact displayed —
    // lands a count on a thread the user is reading, and leaving A must still
    // not reveal it. Nothing but that effect's cleanup clears this.
    useSingleSessionOfProvider(SESSION_ID_2, 'claude', SESSION_2_MAIN_THREAD_ID);
    useNavStore.setState({
      focusedSessionId: SESSION_ID_2,
      activeThreadId: SESSION_2_BRANCH_THREAD_ID,
    });

    const { queryClient } = renderScreen();

    await waitFor(() => expect(threadRow('scratch ideas')).toBeInTheDocument());

    act(() => {
      applySessionEvent(
        {
          kind: 'turn_completed',
          session_id: SESSION_ID_2,
          thread_id: SESSION_2_BRANCH_THREAD_ID,
          stop_reason: null,
        },
        queryClient,
        // The lagging focus state, deliberately not read from the nav store.
        null,
        SESSION_ID_2,
      );
    });
    expect(useLiveStore.getState().unread[SESSION_2_BRANCH_THREAD_ID]).toBe(1);

    fireEvent.click(screen.getByTestId('session-node'));
    await waitFor(() =>
      expect(useNavStore.getState().activeThreadId).toBe(
        SESSION_2_MAIN_THREAD_ID,
      ),
    );

    expect(within(threadRow('scratch ideas')).queryByText('1')).toBeNull();
    expect(
      useLiveStore.getState().unread[SESSION_2_BRANCH_THREAD_ID],
    ).toBeUndefined();
  });

  it('shows the first page of sessions when more remain', async () => {
    renderScreen();

    // The list is cursor-paginated and open-first (mock page size 2): the first
    // page holds every live session — here sess-mock-1 alone — plus the two
    // most-recently-active closed ones; the rest stay unloaded. (The list is
    // windowed, but three rows are well within the rendered window, so all are
    // mounted.)
    await waitFor(() =>
      expect(screen.getAllByTestId('session-node').length).toBe(3),
    );
    // The open one (sess-mock-1) leads page 1; its dot carries the "Open" name.
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
    // is visible but NOT focused. Each row's list item carries its thread tree,
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

  it("shows a branch send's new thread in the focused row once the targeted refetch lands", async () => {
    // The row draws its tree from the session list, but a branch send does not
    // wait for the list: it refetches the focused session's threads alone, and
    // the row reads that same per-session cache entry. The list here keeps
    // answering the pre-branch tree, so the new thread can only reach the row
    // through the targeted refetch.
    const branch: Thread = {
      id: 900,
      session_id: SESSION_ID,
      title: 'the new branch',
      parent_thread_id: MAIN_THREAD_ID,
      root_message_uuid: 'uuid-u1',
      created_at: '2026-01-01T00:10:00Z',
      last_activity_at: null,
    };
    let threadsNow: Thread[] = mockThreads;
    let threadsRequests = 0;
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
              turn: { state: 'idle', send_id: null, thread_id: null },
              running_subagents: [],
              main_thread_id: MAIN_THREAD_ID,
              threads: mockThreads,
              last_activity_at: '2026-01-01T00:00:02Z',
            },
          ],
          next_cursor: null,
        }),
      ),
      http.get(`*/api/sessions/${SESSION_ID}/threads`, () => {
        threadsRequests += 1;
        return HttpResponse.json({ threads: threadsNow });
      }),
      http.post('*/api/sends', () => {
        // The server creates the branch thread as it accepts the send.
        threadsNow = [...mockThreads, branch];
        return HttpResponse.json(
          {
            send: {
              id: 1,
              session_id: SESSION_ID,
              thread_id: branch.id,
              semantic_parent_uuid: 'uuid-u1',
              text: 'branch off here',
              locator_quote: null,
              status: 'dispatched',
              matched_uuid: null,
              created_at: '2026-01-01T00:10:00Z',
              held_at: null,
            },
          },
          { status: 201 },
        );
      }),
    );
    function BranchSender() {
      const createSend = useCreateSendMutation(useApiClient());
      return (
        <button
          type="button"
          onClick={() =>
            createSend.mutate({
              thread_id: MAIN_THREAD_ID,
              text: 'branch off here',
              semantic_parent_uuid: 'uuid-u1',
            })
          }
        >
          send a branch
        </button>
      );
    }
    renderScreen(<BranchSender />);

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
    // The row's `<li>` holds both its header and its thread tree.
    const row = () =>
      screen.getByTestId('session-node').closest('li') as HTMLElement;
    await waitFor(() =>
      expect(within(row()).getByText('delta etymology')).toBeInTheDocument(),
    );
    expect(within(row()).queryByText('the new branch')).not.toBeInTheDocument();
    const requestsBeforeSend = threadsRequests;

    fireEvent.click(screen.getByRole('button', { name: 'send a branch' }));

    await waitFor(() =>
      expect(within(row()).getByText('the new branch')).toBeInTheDocument(),
    );
    expect(threadsRequests).toBeGreaterThan(requestsBeforeSend);
  });

  it('falls back to the most-recently-active session when none are open', async () => {
    // The list is open-first, so with nothing live the whole response is the
    // closed group and arrives recency-descending (newest first); its head is
    // the most recent and gets focused.
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
                pull_request_number: null,
              },
              open: false,
              turn: { state: 'idle', send_id: null, thread_id: null },
              running_subagents: [],
              main_thread_id: SESSION_2_MAIN_THREAD_ID,
              threads: listedThreads(SESSION_ID_2, SESSION_2_MAIN_THREAD_ID),
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
                pull_request_number: null,
              },
              open: false,
              turn: { state: 'idle', send_id: null, thread_id: null },
              running_subagents: [],
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
                pull_request_number: null,
              },
              open: true,
              turn: { state: 'idle', send_id: null, thread_id: null },
              running_subagents: [],
              main_thread_id: MAIN_THREAD_ID,
              threads: listedThreads(SESSION_ID, MAIN_THREAD_ID),
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
});
