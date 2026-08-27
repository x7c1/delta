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
import {
  noticeOf,
  useLiveStore,
  type SpawnItem,
} from '../../store/liveStore';
import { useNotificationStore } from '../../store/notificationStore';
import { PendingQueue } from './PendingQueue';
import { usePendingSends, type PendingSurface } from './usePendingSends';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** The strip as the transcript pane mounts it: rows merged per surface. */
function Strip({
  surface,
  sessionSpawning,
}: {
  surface: PendingSurface;
  sessionSpawning?: boolean;
}) {
  const entries = usePendingSends(surface);
  return <PendingQueue entries={entries} sessionSpawning={sessionSpawning} />;
}

function renderStrip(
  surface: PendingSurface,
  seed?: (queryClient: QueryClient) => void,
  // As the transcript pane relays it: the focused session is still starting.
  sessionSpawning?: boolean,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  seed?.(queryClient);
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <Strip surface={surface} sessionSpawning={sessionSpawning} />
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
    notices: {},
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
    held_at: null,
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

  it('says a queued send waits for the session while it is still starting', () => {
    // The first prompt of a launch that has been accepted but not bound yet
    // (a Codex launch parks it `queued` until `thread/start` answers) is not
    // waiting on a busy session — there is no session yet. "sends when idle"
    // would describe a session that is working on something else, so the row
    // says what it is actually waiting for.
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [
            serverSend({ id: 1, text: 'wake up eventually', status: 'queued' }),
          ],
        });
      },
      true,
    );

    expect(
      screen.getByText('queued — sends when the session starts'),
    ).toBeInTheDocument();
    expect(
      screen.queryByText('queued — sends when idle'),
    ).not.toBeInTheDocument();
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

  it('renders a held send with the neutral label plus explicit Send and Cancel', () => {
    // A queued row with a non-null held_at is held: the server will not
    // dispatch it on its own. It must NOT read as "sends when idle"; instead
    // it carries the neutral held label and an explicit Send alongside the
    // usual Cancel.
    //
    // The boot restore and the echo-deadline park leave a row in exactly this
    // state, and the strip renders it from `held_at` alone — nothing in the
    // row says which one produced it, so the label has to be neutral about the
    // cause: naming the restart would be a lie on a parked row, which explains
    // itself through the `send_parked` notice instead.
    renderStrip(
      { kind: 'thread', sessionId: SESSION_ID, threadId: 1 },
      (queryClient) => {
        queryClient.setQueryData(queryKeys.sessionSends(SESSION_ID), {
          sends: [
            serverSend({
              id: 5,
              text: 'composed before the restart',
              status: 'queued',
              held_at: '2026-01-02T00:00:00Z',
            }),
          ],
        });
      },
    );

    expect(screen.getByText('Held — send or cancel')).toBeInTheDocument();
    expect(
      screen.queryByText('Restored after restart'),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText('queued — sends when idle'),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Send' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  it('Send releases a held send and the refetch clears its held state', async () => {
    // The explicit release: Send hits the release endpoint; the refetch the
    // mutation triggers then reports the row dispatched (the server released
    // it into the normal queued flow and it typed immediately), so the held
    // chip gives way to the ordinary awaiting-reply row.
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
                  held_at: '2026-01-02T00:00:00Z',
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

    await screen.findByText('Held — send or cancel');
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(
        screen.queryByText('Held — send or cancel'),
      ).not.toBeInTheDocument();
    });
    expect(releaseUrls).toEqual(['/api/sends/5/release']);
    // The row is still pending (dispatched now), not gone.
    expect(screen.getByText('held over')).toBeInTheDocument();
    expect(screen.getByText('awaiting reply')).toBeInTheDocument();
    expect(useNotificationStore.getState().errors).toHaveLength(0);
  });

  it('retires the parked-send notice when the row it points at is acted on', async () => {
    // The notice says the message "is waiting in the queue below: send it
    // again … or cancel it". Once the user does either, the row it points at
    // is no longer waiting, so the card must go with it — a cancel especially,
    // which the server broadcasts as nothing at all.
    for (const action of ['Send', 'Cancel'] as const) {
      server.use(
        http.get('*/api/sessions/:id/sends', () =>
          HttpResponse.json({
            sends: [
              serverSend({
                id: 7,
                text: 'swallowed twice',
                status: 'queued',
                held_at: '2026-01-02T00:00:00Z',
              }),
            ],
            turn: { state: 'idle', send_id: null, thread_id: null },
            permission: null,
            question: null,
            running_subagents: [],
          }),
        ),
        http.post('*/api/sends/:id/release', () => new HttpResponse(null, { status: 204 })),
        http.post('*/api/sends/:id/cancel', () => new HttpResponse(null, { status: 204 })),
      );
      useLiveStore.setState({
        notices: {
          [SESSION_ID]: [{ kind: 'send_parked', sendId: 7, at: 0 }],
        },
      });

      const { unmount } = renderStrip({
        kind: 'thread',
        sessionId: SESSION_ID,
        threadId: 1,
      });

      await screen.findByText('swallowed twice');
      fireEvent.click(screen.getByRole('button', { name: action }));

      await waitFor(() => {
        expect(
          noticeOf(useLiveStore.getState().notices, SESSION_ID, 'send_parked'),
        ).toBeNull();
      });
      unmount();
    }
  });

  it('keeps the parked-send notice when the release is refused', async () => {
    // A refused release leaves the row held — `resume_unavailable` says the
    // server never even reached the marker. Retiring the notice on the click
    // would strand the user with a row still sitting in the queue and nothing
    // left saying why it is there, so the card only goes once the server has
    // actually accepted.
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [
            serverSend({
              id: 7,
              text: 'swallowed twice',
              status: 'queued',
              held_at: '2026-01-02T00:00:00Z',
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
          { error: 'session cannot be resumed', code: 'resume_unavailable' },
          { status: 409 },
        ),
      ),
    );
    useLiveStore.setState({
      notices: { [SESSION_ID]: [{ kind: 'send_parked', sendId: 7, at: 0 }] },
    });

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('swallowed twice');
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(useNotificationStore.getState().errors).toHaveLength(1);
    });
    expect(useNotificationStore.getState().errors[0].title).toBe(
      'Could not send the message',
    );
    // The row is still held, and the notice explaining why is still up.
    expect(screen.getByText('Held — send or cancel')).toBeInTheDocument();
    expect(
      noticeOf(useLiveStore.getState().notices, SESSION_ID, 'send_parked'),
    ).not.toBeNull();
  });

  it('keeps the parked-send notice when the cancel is refused', async () => {
    // Same for the other control: a `send_not_cancellable` refusal means the
    // row did not leave the queue, so the card must not disappear ahead of it.
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: [
            serverSend({
              id: 7,
              text: 'swallowed twice',
              status: 'queued',
              held_at: '2026-01-02T00:00:00Z',
            }),
          ],
          turn: { state: 'idle', send_id: null, thread_id: null },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/cancel', () =>
        HttpResponse.json(
          { error: 'not cancellable', code: 'send_not_cancellable' },
          { status: 409 },
        ),
      ),
    );
    useLiveStore.setState({
      notices: { [SESSION_ID]: [{ kind: 'send_parked', sendId: 7, at: 0 }] },
    });

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('swallowed twice');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(useNotificationStore.getState().errors).toHaveLength(1);
    });
    expect(
      noticeOf(useLiveStore.getState().notices, SESSION_ID, 'send_parked'),
    ).not.toBeNull();
  });

  it('leaves another send’s parked notice alone when a held row is cancelled', async () => {
    // The notice names one send. Cancelling a different row — the ordinary
    // case, since the strip lists everything open — must not take it down.
    let cancelled = false;
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends: cancelled
            ? []
            : [serverSend({ id: 8, text: 'unrelated', status: 'queued' })],
          turn: { state: 'idle', send_id: null, thread_id: null },
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
      http.post('*/api/sends/:id/cancel', () => {
        cancelled = true;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    useLiveStore.setState({
      notices: { [SESSION_ID]: [{ kind: 'send_parked', sendId: 7, at: 0 }] },
    });

    renderStrip({ kind: 'thread', sessionId: SESSION_ID, threadId: 1 });

    await screen.findByText('unrelated');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(screen.queryByText('unrelated')).not.toBeInTheDocument();
    });
    expect(
      noticeOf(useLiveStore.getState().notices, SESSION_ID, 'send_parked'),
    ).toMatchObject({ sendId: 7 });
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
              held_at: '2026-01-02T00:00:00Z',
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
  provider: 'claude',
  worktree: null,
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
    // A failure the server could not explain shows the generic line alone,
    // and a launch with nothing queued behind its first prompt has no
    // returned-message line either.
    expect(screen.queryByTestId('pending-fail-reason')).toBeNull();
    expect(screen.queryByTestId('pending-fail-note')).toBeNull();
  });

  // A launch that failed with sends queued behind its first prompt hands those
  // back to the new-session composer — a different screen, with no trace of
  // where they came from. The chip is the only place that can account for
  // them, and for the fact that Retry re-sends its own prompt alone.
  it('says how many messages went back to the composer', () => {
    useLiveStore.setState({ spawns: [{ ...failedSpawn, restoredCount: 1 }] });
    renderStrip({ kind: 'new-session' });

    expect(screen.getByTestId('pending-fail-note')).toHaveTextContent(
      '1 later message was returned to the composer. Retry re-sends only this one.',
    );
  });

  it('pluralizes the returned-message line', () => {
    useLiveStore.setState({ spawns: [{ ...failedSpawn, restoredCount: 3 }] });
    renderStrip({ kind: 'new-session' });

    expect(screen.getByTestId('pending-fail-note')).toHaveTextContent(
      '3 later messages were returned to the composer.',
    );
  });

  it('omits the returned-message line when nothing was returned', () => {
    // The first prompt was all there was: it is on this chip, and the composer
    // holds nothing of this launch's.
    useLiveStore.setState({ spawns: [{ ...failedSpawn, restoredCount: 0 }] });
    renderStrip({ kind: 'new-session' });

    expect(screen.queryByTestId('pending-fail-note')).toBeNull();
  });

  // The launch preparation runs after the send is accepted, so a git or tmux
  // failure has no response body to travel in: the reason the event carries is
  // the only account the user gets of what actually went wrong.
  it('shows the reported reason under the generic failure line', () => {
    useLiveStore.setState({
      spawns: [
        {
          ...failedSpawn,
          reason: 'git error: invalid reference: origin/nope',
        },
      ],
    });
    renderStrip({ kind: 'new-session' });

    expect(screen.getByText(/failed to start/i)).toBeInTheDocument();
    expect(screen.getByTestId('pending-fail-reason')).toHaveTextContent(
      'invalid reference: origin/nope',
    );
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

  it('Retry re-sends the provider and the worktree the failed launch used', async () => {
    // The whole point of Retry is "the same session, again". A Codex launch
    // started from a PR fails in the launch preparation more often than any
    // other (the adapter handshake, the worktree checkout), and retrying it as
    // a Claude session in the plain workdir would start something the user
    // never asked for — silently, since the chip says nothing about which
    // session it is about to start.
    let captured: unknown;
    server.use(
      http.post('*/api/sends', async ({ request }) => {
        captured = await request.json();
        return HttpResponse.json(
          {
            send: {
              id: 11,
              session_id: 'sess-retried',
              thread_id: 12,
              semantic_parent_uuid: null,
              text: 'start a new session',
              locator_quote: null,
              status: 'queued',
              matched_uuid: null,
              created_at: '2026-01-01T00:00:00Z',
              held_at: null,
            },
          },
          { status: 201 },
        );
      }),
    );
    useLiveStore.setState({
      spawns: [
        {
          ...failedSpawn,
          provider: 'codex',
          worktree: {
            start_point: { kind: 'use_remote_branch', name: 'feature/x' },
          },
        },
      ],
    });
    renderStrip({ kind: 'new-session' });

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => {
      expect(captured).toEqual({
        new_session: true,
        text: 'start a new session',
        workdir: '/work/dir',
        launch_option_ids: [2, 5],
        worktree: {
          start_point: { kind: 'use_remote_branch', name: 'feature/x' },
        },
        provider: 'codex',
      });
    });
    // …and the freshly tracked spawn keeps them too, so a second failure can
    // be retried the same way.
    const fresh = useLiveStore.getState().spawns[0];
    expect(fresh.provider).toBe('codex');
    expect(fresh.worktree).toEqual({
      start_point: { kind: 'use_remote_branch', name: 'feature/x' },
    });
  });
});

describe('PendingQueue failed submit', () => {
  beforeEach(reset);

  it('shows the generic line alone when the server named nothing', () => {
    useLiveStore.setState({
      sending: [
        {
          id: 'l1',
          target: {
            kind: 'new-session',
            workdir: null,
            launchOptionIds: [7],
            provider: 'claude',
            worktree: null,
          },
          text: 'start a new session',
          status: 'failed',
          createdAt: 0,
        },
      ],
    });
    renderStrip({ kind: 'new-session' });

    expect(screen.getByText(/failed to start/i)).toBeInTheDocument();
    expect(screen.queryByTestId('pending-fail-reason')).toBeNull();
  });

  // A refused launch option is the one send rejection whose message says
  // something the chip's own copy cannot: which selected option, or which
  // merged-`config` key path, the server would not apply. Swallowing it would
  // leave the user guessing which of their ticked rows to fix.
  it('shows a refused launch option message verbatim under the generic line', () => {
    const message =
      'launch option rejected: the selected `config` options disagree: ' +
      '`sandbox_workspace_write.writable_roots` is set to both ["/a"] and "/b"';
    useLiveStore.setState({
      sending: [
        {
          id: 'l1',
          target: {
            kind: 'new-session',
            workdir: null,
            launchOptionIds: [7],
            provider: 'claude',
            worktree: null,
          },
          text: 'start a new session',
          status: 'failed',
          createdAt: 0,
          reason: message,
        },
      ],
    });
    renderStrip({ kind: 'new-session' });

    expect(screen.getByTestId('pending-fail-reason')).toHaveTextContent(
      message,
    );
  });
});
