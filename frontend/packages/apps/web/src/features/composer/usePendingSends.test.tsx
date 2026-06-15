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
import { useLiveStore } from '../../store/liveStore';
import { usePendingSends, type PendingSurface } from './usePendingSends';

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
