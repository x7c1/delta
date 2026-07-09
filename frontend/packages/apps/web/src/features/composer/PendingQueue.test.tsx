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
import { http, HttpResponse } from 'msw';
import { createHandlers, mockSpawnSessionId, SESSION_ID } from '@delta/api-mocks';
import { ApiClient, queryKeys } from '@delta/api-client';
import type { Send } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore, type SpawnItem } from '../../store/liveStore';
import { useNotificationStore } from '../../store/notificationStore';
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
  useNotificationStore.setState({ errors: [] });
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
    restored_at: null,
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

  it('cancels a dispatched send whose echo never arrived, clearing the strip', async () => {
    // The user pressed Escape in the TUI to discard the composer buffer, so
    // no `UserPromptSubmit` ever fires and the row would otherwise stay
    // `dispatched` indefinitely. The Cancel button on the dispatched row is
    // the escape hatch: the server injects Escape on the user's behalf,
    // drops the row to `cancelled`, and the refetch the mutation triggers
    // clears the chip.
    let cancelled = false;
    const cancelUrls: string[] = [];
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: cancelled
            ? []
            : [serverSend({ id: 99, text: 'stuck', status: 'dispatched' })],
          turn: cancelled
            ? { state: 'idle', send_id: null, thread_id: null }
            : { state: 'awaiting_echo', send_id: 99, thread_id: 1 },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/cancel', ({ request, params }) => {
        cancelUrls.push(new URL(request.url).pathname);
        if (params.id === '99') {
          cancelled = true;
          return new HttpResponse(null, { status: 204 });
        }
        return HttpResponse.json(
          { error: 'not cancellable', code: 'send_not_cancellable' },
          { status: 409 },
        );
      }),
    );

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('stuck');
    // The dispatched row shows the "awaiting reply" spinner alongside the
    // Cancel control: same gesture as a queued cancel, different server path.
    expect(screen.getByText('awaiting reply')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(screen.queryByText('stuck')).not.toBeInTheDocument();
    });
    expect(cancelUrls).toEqual(['/api/sends/99/cancel']);
    expect(screen.queryAllByTestId('pending-item')).toHaveLength(0);
  });

  it('cancels a queued send, removing it from the strip', async () => {
    // Override the open-send + cancel routes for this test so the flow is
    // self-contained: the first sends fetch carries the queued row, Cancel hits
    // the send-scoped cancel route, and the refetch the mutation triggers then
    // returns an empty list (the row was cancelled server-side).
    let cancelled = false;
    const cancelUrls: string[] = [];
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: cancelled
            ? []
            : [serverSend({ id: 42, text: 'parked', status: 'queued' })],
          turn: { state: 'idle', send_id: null, thread_id: null },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/cancel', ({ request, params }) => {
        cancelUrls.push(new URL(request.url).pathname);
        if (params.id === '42') {
          cancelled = true;
          return new HttpResponse(null, { status: 204 });
        }
        return HttpResponse.json(
          { error: 'not cancellable', code: 'send_not_cancellable' },
          { status: 409 },
        );
      }),
    );

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('parked');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(screen.queryByText('parked')).not.toBeInTheDocument();
    });
    expect(cancelUrls).toEqual(['/api/sends/42/cancel']);
    expect(screen.queryAllByTestId('pending-item')).toHaveLength(0);
  });

  it('surfaces a refused cancel through the notification store instead of failing silently', async () => {
    // The server refuses the cancel (`409 send_not_cancellable`) — e.g. the
    // echo already arrived so the turn owns the send. Before this test's
    // feature, the mutation only invalidated the open-send list, so the
    // refusal produced no user-visible feedback and the Cancel button read
    // as dead. Now the failure pushes an explanation onto the app-wide
    // notification store (rendered by `ErrorSnackbar`).
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [serverSend({ id: 7, text: 'unyielding', status: 'dispatched' })],
          turn: { state: 'in_flight', send_id: 7, thread_id: 1 },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/cancel', () =>
        HttpResponse.json(
          { error: 'send 7 is not cancellable', code: 'send_not_cancellable' },
          { status: 409 },
        ),
      ),
    );

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('unyielding');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(useNotificationStore.getState().errors).toHaveLength(1);
    });
    const [notice] = useNotificationStore.getState().errors;
    expect(notice.title).toBe('Could not cancel the send');
    expect(notice.detail).toMatch(/no longer cancellable/);
    // The chip stays (the refetch still reports the row): the refusal is
    // explained rather than looking like a silently dead button.
    expect(screen.getByText('unyielding')).toBeInTheDocument();
  });

  it('renders a restored send with its label plus explicit Send and Cancel', () => {
    // A queued row with a non-null restored_at was recovered at the server's
    // boot from a dead process's dispatched state. It must NOT read as
    // "sends when idle" (the server never auto-sends it); instead it carries
    // the restored label and an explicit Send alongside the usual Cancel.
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [
            serverSend({
              id: 5,
              text: 'composed before the restart',
              status: 'queued',
              restored_at: '2026-01-02T00:00:00Z',
            }),
          ],
        });
      },
    );

    expect(screen.getByText('Restored after restart')).toBeInTheDocument();
    expect(
      screen.queryByText('queued — sends when idle'),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Send' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  it('Send releases a restored send and the refetch clears its restored state', async () => {
    // The explicit release: Send hits the release endpoint; the refetch the
    // mutation triggers then reports the row dispatched (the server released
    // it into the normal queued flow and it typed immediately), so the
    // restored chip gives way to the ordinary awaiting-reply row.
    let released = false;
    const releaseUrls: string[] = [];
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [
            released
              ? serverSend({ id: 5, text: 'held over', status: 'dispatched' })
              : serverSend({
                  id: 5,
                  text: 'held over',
                  status: 'queued',
                  restored_at: '2026-01-02T00:00:00Z',
                }),
          ],
          turn: released
            ? { state: 'awaiting_echo', send_id: 5, thread_id: 1 }
            : { state: 'idle', send_id: null, thread_id: null },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/release', ({ request, params }) => {
        releaseUrls.push(new URL(request.url).pathname);
        if (params.id === '5') {
          released = true;
          return new HttpResponse(null, { status: 204 });
        }
        return HttpResponse.json(
          { error: 'not releasable', code: 'send_not_releasable' },
          { status: 409 },
        );
      }),
    );

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('Restored after restart');
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(
        screen.queryByText('Restored after restart'),
      ).not.toBeInTheDocument();
    });
    expect(releaseUrls).toEqual(['/api/sends/5/release']);
    // The row is still pending (dispatched now), not gone.
    expect(screen.getByText('held over')).toBeInTheDocument();
    expect(screen.getByText('awaiting reply')).toBeInTheDocument();
    expect(useNotificationStore.getState().errors).toHaveLength(0);
  });

  it('surfaces a refused release through the notification store instead of failing silently', async () => {
    // The server refuses the release (409 send_not_releasable) — e.g. the
    // row was cancelled from another tab. The failure pushes an explanation
    // onto the app-wide notification store, mirroring the refused-cancel
    // path, so the Send button never reads as dead.
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [
            serverSend({
              id: 6,
              text: 'contested',
              status: 'queued',
              restored_at: '2026-01-02T00:00:00Z',
            }),
          ],
          turn: { state: 'idle', send_id: null, thread_id: null },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/release', () =>
        HttpResponse.json(
          {
            error: 'send 6 is not awaiting a release',
            code: 'send_not_releasable',
          },
          { status: 409 },
        ),
      ),
    );

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('contested');
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(useNotificationStore.getState().errors).toHaveLength(1);
    });
    const [notice] = useNotificationStore.getState().errors;
    expect(notice.title).toBe('Could not send the message');
    expect(notice.detail).toMatch(/no longer awaiting a release/);
    // The chip stays (the refetch still reports the row): the refusal is
    // explained rather than looking like a silently dead button.
    expect(screen.getByText('contested')).toBeInTheDocument();
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
