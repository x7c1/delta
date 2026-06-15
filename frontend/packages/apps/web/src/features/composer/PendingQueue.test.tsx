import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers, mockSpawnSessionId, SESSION_ID } from '@delta/api-mocks';
import { ApiClient, queryKeys } from '@delta/api-client';
import type { Send } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore, type SpawnItem } from '../../store/liveStore';
import { PendingQueue } from './PendingQueue';
import { usePendingSends, type PendingSurface } from './usePendingSends';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** The strip as the transcript pane mounts it: rows merged per surface. */
function Strip({ surface }: { surface: PendingSurface }) {
  const entries = usePendingSends(surface);
  return <PendingQueue entries={entries} />;
}

function renderStrip(
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
        <Strip surface={surface} />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

function reset() {
  useLiveStore.setState({
    sending: [],
    localSends: {},
    spawns: [],
    activeTurns: {},
  });
}

function serverSend(overrides: Partial<Send> = {}): Send {
  return {
    id: 1,
    session_id: SESSION_ID,
    thread_id: 1,
    semantic_parent_uuid: null,
    text: 'a send',
    locator_quote: null,
    status: 'dispatched',
    matched_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('PendingQueue server sends', () => {
  beforeEach(reset);

  it('labels a queued send as deliberate waiting, distinct from a dispatched one', () => {
    // A deferred (queued) send used to look like a failure and caused
    // duplicate resubmits; with server authority the truthful status renders:
    // queued = parked until idle, dispatched = on its way.
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [
            serverSend({ id: 1, text: 'on its way', status: 'dispatched' }),
            serverSend({ id: 2, text: 'parked until idle', status: 'queued' }),
          ],
        });
      },
    );

    expect(screen.getAllByTestId('pending-item')).toHaveLength(2);
    expect(screen.getByText('queued — sends when idle')).toBeInTheDocument();
    expect(screen.getByText('awaiting reply')).toBeInTheDocument();
    expect(screen.getByText('1 queued')).toBeInTheDocument();
  });

  it('shows only the active thread’s sends', () => {
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [serverSend({ id: 3, thread_id: 2, text: 'other thread' })],
        });
      },
    );

    expect(screen.queryAllByTestId('pending-item')).toHaveLength(0);
  });

  it('keeps an in-progress chip for a tracked send that left the open list', () => {
    // The send matched its transcript line (the server list is empty again),
    // but its turn has not ended: the tracked local twin keeps the chip up.
    useLiveStore.getState().recordLocalSend({
      sendId: 7,
      sessionId: SESSION_ID,
      threadId: 1,
      text: 'still running',
      createdAt: 0,
    });
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [],
        });
      },
    );

    const items = screen.getAllByTestId('pending-item');
    expect(items).toHaveLength(1);
    expect(screen.getByText('still running')).toBeInTheDocument();
    // The in-progress indicator now lives in the strip header, not the row:
    // the running row carries no per-row spinner, so its text never shifts.
    expect(
      screen.getByRole('status', { name: 'in progress' }),
    ).toBeInTheDocument();
    expect(items[0].querySelector('[role="status"]')).toBeNull();
  });

  it('does not double-render a tracked send that is still in the open list', () => {
    useLiveStore.getState().recordLocalSend({
      sendId: 1,
      sessionId: SESSION_ID,
      threadId: 1,
      text: 'a send',
      createdAt: 0,
    });
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [serverSend({ id: 1 })],
        });
      },
    );

    // The server row wins while it exists; the local twin only takes over
    // once the send leaves the open list.
    expect(screen.getAllByTestId('pending-item')).toHaveLength(1);
  });
});

const failedSpawn: SpawnItem = {
  sessionId: 'sess-spawn-reaped',
  threadId: 42,
  text: 'start a new session',
  workdir: '/work/dir',
  launchOptionIds: [2, 5],
  status: 'failed',
};

describe('PendingQueue failed spawn', () => {
  beforeEach(reset);

  it('renders a failed spawn with an error message plus Retry and Dismiss', () => {
    useLiveStore.setState({ spawns: [failedSpawn] });
    renderStrip({ kind: 'new-session' });

    expect(screen.getByText(/failed to start/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Dismiss' }),
    ).toBeInTheDocument();
  });

  it('Dismiss removes the failed chip', () => {
    useLiveStore.setState({ spawns: [failedSpawn] });
    renderStrip({ kind: 'new-session' });

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(useLiveStore.getState().spawns).toHaveLength(0);
  });

  it('Retry drops the failed chip and launches a fresh identical spawn', async () => {
    useLiveStore.setState({ spawns: [failedSpawn] });
    renderStrip({ kind: 'new-session' });

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    // The failed spawn is removed, and the fresh attempt is tracked under the
    // REAL ids the mock server mints — same text, same chosen directory.
    await waitFor(() => {
      const spawns = useLiveStore.getState().spawns;
      expect(spawns).toHaveLength(1);
      expect(spawns[0].status).toBe('spawning');
    });
    const fresh = useLiveStore.getState().spawns[0];
    expect(fresh.text).toBe('start a new session');
    expect(fresh.workdir).toBe('/work/dir');
    // The retried launch carries the same selected options, in order.
    expect(fresh.launchOptionIds).toEqual([2, 5]);
    expect(fresh.sessionId).toBe(mockSpawnSessionId(1));
    // The accepted first send is tracked, so the chip stays through the turn.
    const locals = Object.values(useLiveStore.getState().localSends);
    expect(locals).toHaveLength(1);
    expect(locals[0].sessionId).toBe(mockSpawnSessionId(1));
  });
});
