/**
 * Restoring the navigator's running spinners from the session list: a session
 * that is not open gets its spinner back from the list's `turn` when the
 * store starts empty (a reload), and a list requested before a turn ends never
 * brings back the spinner that turn's end put out.
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
import { act, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import {
  MAIN_THREAD_ID,
  SESSION_2_MAIN_THREAD_ID,
  SESSION_ID,
  SESSION_ID_2,
} from '@delta/api-mocks';
import { queryKeys } from '@delta/api-client';
import type { RunningSubagent, Turn } from '@delta/wire-gen';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import {
  deliverEvent,
  renderScreen,
  resetWorkspaceState,
  server,
  singleSessionList,
  stubMatchMedia,
} from './WorkspaceScreen.testkit';

// The live event source opens a real WebSocket outside mock mode, and the
// terminal pane drives xterm.js — neither is meaningful in jsdom. Stub both.
vi.mock('../../data/useSessionEvents', () => ({
  useSessionEvents: () => {},
}));
vi.mock('../terminal/TerminalPane', () => ({
  TerminalPane: () => null,
}));

beforeAll(stubMatchMedia);

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const IN_FLIGHT: Turn = {
  state: 'in_flight',
  send_id: null,
  thread_id: SESSION_2_MAIN_THREAD_ID,
};
const IDLE: Turn = { state: 'idle', send_id: null, thread_id: null };

/**
 * A list of two open sessions: the focused one (idle) first, then a second
 * one, not focused, reporting `turn` and `runningSubagents`.
 */
function twoSessionList(
  turn: Turn,
  runningSubagents: RunningSubagent[] = [],
) {
  return {
    sessions: [
      ...singleSessionList(SESSION_ID, 'claude', MAIN_THREAD_ID).sessions,
      ...singleSessionList(SESSION_ID_2, 'codex', SESSION_2_MAIN_THREAD_ID, {
        turn,
        runningSubagents,
      }).sessions,
    ],
    next_cursor: null,
  };
}

/** The navigator card of the session titled `title`. */
function card(title: string): HTMLElement {
  const found = screen
    .getAllByTestId('session-card')
    .find((element) => within(element).queryByText(title) !== null);
  if (found === undefined) {
    throw new Error(`no session card titled "${title}"`);
  }
  return found;
}

describe('WorkspaceScreen running spinners from the session list', () => {
  beforeEach(() => {
    resetWorkspaceState();
    // The reload case: nothing event-built survives, so the store starts empty.
    useLiveStore.setState({ runningThreads: {}, runningSubagents: {} });
    useNavStore.setState({
      focusedSessionId: SESSION_ID,
      activeThreadId: MAIN_THREAD_ID,
    });
  });

  it('lights the spinner of a session that is not focused from its listed turn, with an empty store', async () => {
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json(twoSessionList(IN_FLIGHT)),
      ),
    );

    renderScreen();

    await waitFor(() => {
      expect(
        within(card('codex session')).getByTestId('session-running'),
      ).toBeInTheDocument();
    });
    // The focused session is idle, and the other session never reached
    // `/sends` (only the open session's envelope is fetched): the list alone
    // lit it.
    expect(
      within(card('claude session')).queryByTestId('session-running'),
    ).not.toBeInTheDocument();
    expect(useLiveStore.getState().runningThreads).toEqual({
      [SESSION_ID_2]: { [SESSION_2_MAIN_THREAD_ID]: true },
    });
  });

  it('seeds the running subagents of a session that is not focused from its listed row, with an empty store', async () => {
    // A background subagent outlives its launching turn, so the row is idle
    // and only `running_subagents` says something is still running.
    const subagent: RunningSubagent = {
      thread_id: SESSION_2_MAIN_THREAD_ID,
      tool_use_id: 'toolu_listed',
      subagent_type: 'general-purpose',
      description: null,
      background: true,
    };
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json(twoSessionList(IDLE, [subagent])),
      ),
    );

    renderScreen();

    await waitFor(() => {
      expect(
        useLiveStore
          .getState()
          .runningSubagents[SESSION_ID_2]?.map((s) => s.toolUseId),
      ).toEqual(['toolu_listed']);
    });
    expect(useLiveStore.getState().runningSubagents[SESSION_ID]).toBeUndefined();
  });

  it('does not bring the spinner back from a list requested before the turn completed', async () => {
    // First answer: the second session's turn is in flight.
    let answer: 'running' | 'stale' | 'idle' = 'running';
    let releaseStale: () => void = () => {};
    const staleGate = new Promise<void>((resolve) => {
      releaseStale = resolve;
    });
    server.use(
      http.get('*/api/sessions', async () => {
        if (answer === 'running') {
          return HttpResponse.json(twoSessionList(IN_FLIGHT));
        }
        if (answer === 'stale') {
          // Assembled while the turn was still running, and held on the wire
          // until after its `turn_completed` has landed. Every later request
          // sees the finished turn.
          answer = 'idle';
          await staleGate;
          return HttpResponse.json(twoSessionList(IN_FLIGHT));
        }
        return HttpResponse.json(twoSessionList(IDLE));
      }),
    );

    const { queryClient } = renderScreen();
    await waitFor(() => {
      expect(
        within(card('codex session')).getByTestId('session-running'),
      ).toBeInTheDocument();
    });

    // A refetch of the list goes out (any list invalidation would do) and is
    // answered from the server's state before the turn ends.
    answer = 'stale';
    act(() => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    });
    await waitFor(() => {
      expect(answer).toBe('idle');
    });

    // The turn completes while that answer is still on the wire.
    deliverEvent(queryClient, {
      kind: 'turn_completed',
      session_id: SESSION_ID_2,
      thread_id: SESSION_2_MAIN_THREAD_ID,
      stop_reason: null,
    });
    expect(
      within(card('codex session')).queryByTestId('session-running'),
    ).not.toBeInTheDocument();

    // The stale answer arrives; then the list settles.
    releaseStale();
    await waitFor(() => {
      expect(
        queryClient.getQueryState(queryKeys.sessions)?.fetchStatus,
      ).toBe('idle');
    });
    expect(
      within(card('codex session')).queryByTestId('session-running'),
    ).not.toBeInTheDocument();
    expect(useLiveStore.getState().runningThreads[SESSION_ID_2]).toBeUndefined();
  });
});
