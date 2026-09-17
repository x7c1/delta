import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, SESSION_ID } from '@delta/api-mocks';
import { ApiClient, queryKeys } from '@delta/api-client';
import type { SendsResponse, Turn } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore, type SendingItem } from '../../store/liveStore';
import {
  usePendingSends,
  type PendingEntry,
  type PendingSurface,
} from './usePendingSends';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** A bare driver that only runs the hook's seeding effects. */
function Driver({ surface }: { surface: PendingSurface }) {
  usePendingSends(surface);
  return null;
}

function mount(
  surface: PendingSurface,
  seed?: (queryClient: QueryClient) => void,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  seed?.(queryClient);
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <Driver surface={surface} />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/**
 * Mount the hook and keep the latest entries it returns in a plain box, so a
 * test can assert on the merged rows themselves rather than on whatever a
 * component happens to render from them.
 */
function captureEntries(surface: PendingSurface): { current: PendingEntry[] } {
  const box: { current: PendingEntry[] } = { current: [] };
  function Capture() {
    box.current = usePendingSends(surface);
    return null;
  }
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <Capture />
      </ApiProvider>
    </QueryClientProvider>,
  );
  return box;
}

function reset() {
  useLiveStore.setState({
    sending: [],
    localSends: {},
    spawns: [],
    runningThreads: {},
  });
}

const THREAD_SURFACE: PendingSurface = {
  kind: 'thread',
  sessionId: SESSION_ID,
  threadId: 1,
};

const inFlightEnvelope: SendsResponse = {
  sends: [],
  turn: { state: 'in_flight', send_id: 1, thread_id: 1 } satisfies Turn,
  permission: null,
  question: null,
};

describe('usePendingSends active-turn seeding', () => {
  beforeEach(reset);

  it('does not leave the running flag stuck on after a turn completes off-focus', async () => {
    // Repro of the stuck-running-spinner leak:
    //  1. The session ran and was focused, so its sends cache holds a stale
    //     `turn: in_flight` envelope.
    //  2. Its turn completed while a DIFFERENT session was focused, so
    //     `turn_completed` already cleared `activeTurns[S]`.
    //  3. Re-focusing S mounts this query. React Query serves the stale cached
    //     `in_flight` first (set-only re-seed could resurrect the flag) and
    //     then refetches the fresh `turn: idle` from the server.
    // The fresh idle must win and leave the flag cleared.
    mount(THREAD_SURFACE, (queryClient) => {
      queryClient.setQueryData(
        queryKeys.sessionSends(SESSION_ID),
        inFlightEnvelope,
      );
    });

    // The stale read is a set-only no-op while the refetch is in flight; once
    // the fresh `idle` lands it authoritatively clears the flag.
    await waitFor(() => {
      expect(useLiveStore.getState().runningThreads).toEqual({});
    });
  });

  it('heals a dropped flag when the fresh fetch is genuinely in_flight', async () => {
    // Reconnect healing must survive: when the resync refetch lands a real
    // `in_flight`, the flag the reset dropped is re-set. Override the sends
    // handler so the server itself reports a live `in_flight` turn (the default
    // mock can only report `awaiting_echo`/`idle`).
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json(inFlightEnvelope),
      ),
    );

    mount(THREAD_SURFACE);

    await waitFor(() => {
      expect(useLiveStore.getState().runningThreads).toEqual({
        [SESSION_ID]: { 1: true },
      });
    });
  });
});

describe('usePendingSends on a starting session', () => {
  beforeEach(reset);

  it('renders the first prompt once, not twice', async () => {
    // A session is focused the moment its first send is accepted, so its
    // thread surface is on screen while the launch comes up. That one prompt
    // has two client-side records — the server's open-send row (`dispatched`,
    // from `GET /api/sessions/{id}/sends`) and the local send `useSubmitSend`
    // tracked from the same POST response — and they carry the SAME send id.
    // The strip must merge them into one row; a duplicate here would show the
    // user their message twice for the whole spawn window.
    const send = {
      id: 77,
      session_id: SESSION_ID,
      thread_id: 1,
      semantic_parent_uuid: null,
      text: 'first message',
      locator_quote: null,
      status: 'dispatched' as const,
      matched_uuid: null,
      created_at: '2026-01-01T00:00:00Z',
    };
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [send],
          turn: { state: 'idle' } satisfies Turn,
          permission: null,
          question: null,
        } satisfies SendsResponse),
      ),
    );
    useLiveStore.getState().recordLocalSend({
      sendId: send.id,
      sessionId: SESSION_ID,
      threadId: 1,
      text: send.text,
      createdAt: 0,
    });

    const entries = captureEntries(THREAD_SURFACE);

    // Wait for the server row to land (before it does, only the local twin is
    // there), then assert it did not double up with the local one.
    await waitFor(() => expect(entries.current[0]?.kind).toBe('server'));
    expect(entries.current).toHaveLength(1);
  });
});

describe('usePendingSends on the new-session surface', () => {
  beforeEach(reset);

  const NEW_SESSION_SURFACE: PendingSurface = { kind: 'new-session' };

  const LAUNCH = {
    workdir: null,
    launchOptionIds: [],
    provider: 'claude' as const,
    worktree: null,
    pullRequestNumber: null,
  };

  const FIRST_SEND = {
    sendId: 42,
    sessionId: SESSION_ID,
    threadId: 1,
    text: 'start something',
    createdAt: 0,
  };

  /** A launch that was accepted and is still coming up, with its first send. */
  function trackStartingSession() {
    useLiveStore.setState({
      spawns: [
        {
          ...LAUNCH,
          sessionId: SESSION_ID,
          threadId: FIRST_SEND.threadId,
          text: FIRST_SEND.text,
          firstSendId: FIRST_SEND.sendId,
          status: 'spawning',
        },
      ],
    });
    useLiveStore.getState().recordLocalSend(FIRST_SEND);
  }

  it('lists nothing for the first send of a session that is still starting', () => {
    // The user started a session, walked away while it launched, and pressed
    // New session. The screen they are about to write the next prompt on says
    // nothing about the session they already started: that one has its own row
    // and its own screen, where its first prompt is shown.
    trackStartingSession();

    const entries = captureEntries(NEW_SESSION_SURFACE);

    expect(entries.current).toEqual([]);
  });

  it('still shows a starting session’s first send on its own thread surface', () => {
    // The other half of the same rule: removed from the new-session screen,
    // kept where it belongs. The server's open list is empty here (the send
    // already matched, or the launch has not bound), so the tracked twin is
    // the only thing that can carry the row.
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [],
          turn: { state: 'idle' } satisfies Turn,
          permission: null,
          question: null,
        } satisfies SendsResponse),
      ),
    );
    trackStartingSession();

    const entries = captureEntries(THREAD_SURFACE);

    expect(entries.current).toEqual([
      {
        kind: 'local',
        key: `local-${FIRST_SEND.sendId}`,
        send: FIRST_SEND,
      },
    ]);
  });

  it('lists a new-session submit whose POST is still in flight', () => {
    // What the new-session screen does still own: the send it is itself in the
    // middle of making. A starting session alongside it changes nothing.
    const submit: SendingItem = {
      id: 'submit-1',
      target: { kind: 'new-session', ...LAUNCH },
      text: 'and now the next one',
      status: 'sending',
      createdAt: 0,
    };
    trackStartingSession();
    useLiveStore.setState({ sending: [submit] });

    const entries = captureEntries(NEW_SESSION_SURFACE);

    expect(entries.current).toEqual([
      { kind: 'sending', key: submit.id, item: submit },
    ]);
  });
});
