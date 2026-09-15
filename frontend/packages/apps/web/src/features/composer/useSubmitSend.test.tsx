import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { act, render } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, SESSION_ID } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SendResponse } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { newSessionSendBody } from './newSessionRequest';
import { useSubmitSend } from './useSubmitSend';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** The id the gated handler below accepts the send into. */
const SPAWNED_SESSION_ID = 'sess-just-spawned';

const LAUNCH = {
  workdir: '/work',
  launchOptionIds: [] as number[],
  provider: 'claude' as const,
  worktree: null,
  pullRequestNumber: null,
};

/**
 * A bare driver that hands the submit callback to the test. Nothing else is
 * mounted — no workspace, hence no hand-over effect — so whatever the spawn
 * records here was decided by the submit path alone, at the moment the POST
 * resolved.
 */
function mountSubmit(): { submit: () => Promise<unknown> } {
  const box: { current: ReturnType<typeof useSubmitSend> | null } = {
    current: null,
  };
  function Driver() {
    box.current = useSubmitSend();
    return null;
  }
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
        <Driver />
      </ApiProvider>
    </QueryClientProvider>,
  );
  return {
    submit: () => {
      const submitSend = box.current;
      if (submitSend === null) {
        throw new Error('submit callback not mounted');
      }
      return submitSend({
        target: { kind: 'new-session', ...LAUNCH },
        text: 'start fresh',
        body: newSessionSendBody('start fresh', LAUNCH),
      });
    },
  };
}

/**
 * Hold `POST /api/sends` open until the returned callback is invoked, then
 * accept it as the server does for a new session (real ids on the response).
 * The wait is the window in which the user can navigate — the whole point of
 * deciding the hand-over from where they are when the answer lands.
 */
function gateSends(): () => void {
  let open: () => void = () => {};
  const gate = new Promise<void>((resolve) => {
    open = resolve;
  });
  server.use(
    http.post('*/api/sends', async () => {
      await gate;
      const body: SendResponse = {
        send: {
          id: 1,
          session_id: SPAWNED_SESSION_ID,
          thread_id: 42,
          semantic_parent_uuid: null,
          text: 'start fresh',
          locator_quote: null,
          status: 'dispatched',
          matched_uuid: null,
          created_at: '2026-01-01T00:00:00Z',
          held_at: null,
        },
      };
      return HttpResponse.json(body, { status: 201 });
    }),
  );
  return open;
}

describe('useSubmitSend spawn focus hand-over', () => {
  beforeEach(() => {
    useLiveStore.setState({ sending: [], localSends: {}, spawns: [] });
    useNavStore.setState({
      focusedSessionId: null,
      activeThreadId: null,
      preNewSessionFocus: null,
    });
  });

  it('owes the hand-over when the response lands on the new-session screen', async () => {
    // The user pressed Send and stayed put: they are waiting to be carried into
    // the session they just started, so the spawn is tracked with its hand-over
    // still owed — the workspace consumes it and moves focus.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    const { submit } = mountSubmit();

    await act(async () => {
      await submit();
    });

    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({ status: 'spawning', focusHandedOver: false }),
    ]);
  });

  it('spends the hand-over when the response lands with the user elsewhere', async () => {
    // Nobody is waiting for this hand-over: the user moved to another session
    // while the POST was travelling. It is spent HERE, at the response — not
    // left pending for the next time the new-session screen opens — and no
    // workspace render was needed to decide that (none is mounted).
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    const { submit } = mountSubmit();

    await act(async () => {
      await submit();
    });

    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({ status: 'spawning', focusHandedOver: true }),
    ]);
    // And the send itself never touched focus.
    expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID);
  });

  it('reads the focus as it stands when the POST resolves, not when it was sent', async () => {
    // The decision belongs to the moment the server answers. Sent from the
    // new-session screen, answered after the user had walked away: the
    // hand-over is spent, because by then there was nobody on that screen to
    // hand anything to.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    const openGate = gateSends();
    const { submit } = mountSubmit();

    let settled: Promise<unknown> = Promise.resolve();
    act(() => {
      settled = submit();
    });
    act(() => {
      useNavStore.setState({ focusedSessionId: SESSION_ID });
    });
    await act(async () => {
      openGate();
      await settled;
    });

    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({ focusHandedOver: true }),
    ]);
  });

  it('owes the hand-over when the user returns to the new-session screen before the response', async () => {
    // The mirror image, and why a value captured at send time will not do: the
    // user stepped into another session while the POST was travelling and came
    // back to the new-session screen before the answer arrived — so they ARE
    // waiting there, and the hand-over is owed. Focus is set away from that
    // screen at submit time too, which is stricter than any route the UI
    // offers (both the new-session composer and a failed spawn's Retry sit on
    // that screen): it pins the decision to the response and nothing else.
    useNavStore.setState({ focusedSessionId: SESSION_ID });
    const openGate = gateSends();
    const { submit } = mountSubmit();

    let settled: Promise<unknown> = Promise.resolve();
    act(() => {
      settled = submit();
    });
    act(() => {
      useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    });
    await act(async () => {
      openGate();
      await settled;
    });

    expect(useLiveStore.getState().spawns).toEqual([
      expect.objectContaining({ focusHandedOver: false }),
    ]);
  });
});
