import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { SESSION_ID } from '@delta/api-mocks';
import type { Send, Turn } from '@delta/wire-gen';
import { useLiveStore } from '../../store/liveStore';
import { useNotificationStore } from '../../store/notificationStore';
import type { SendActivitySurface } from './useSendActivity';
import {
  renderStrip,
  reset,
  server,
  serverSend,
} from './SendActivityStrip.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('SendActivityStrip "Stop"', () => {
  beforeEach(reset);

  const surface: SendActivitySurface = {
    kind: 'thread',
    sessionId: SESSION_ID,
    threadId: 1,
  };

  /**
   * Serve the session's sends envelope: `sends` as the open list and `turn` as
   * the server's turn state, which the strip's running flag re-seeds from.
   */
  function serveSends(sends: Send[], turn: Turn) {
    server.use(
      http.get('*/api/sessions/:id/sends', () =>
        HttpResponse.json({
          sends,
          turn,
          permission: null,
          question: null,
          running_subagents: [],
        }),
      ),
    );
  }

  /**
   * The stuck state: the turn's send already matched (its tracked twin keeps
   * its row up), the turn is in flight, and a send waits queued behind it.
   */
  function serveStuckTurn() {
    useLiveStore.getState().recordLocalSend({
      sendId: 7,
      sessionId: SESSION_ID,
      threadId: 1,
      text: 'cancelled in the pane',
      createdAt: 0,
    });
    serveSends([serverSend({ id: 8, text: 'waiting behind', status: 'queued' })], {
      state: 'in_flight',
      send_id: 7,
      thread_id: 1,
    });
  }

  const stopButton = () => screen.queryByRole('button', { name: 'Stop' });

  it('offers the control for a Claude turn in progress and says it interrupts Claude', async () => {
    serveStuckTurn();
    let stopped: string | null = null;
    server.use(
      http.post('*/api/sessions/:id/stop-turn', ({ params }) => {
        stopped = String(params.id);
        return new HttpResponse(null, { status: 204 });
      }),
    );

    renderStrip(surface, undefined, false, true);

    const button = await screen.findByRole('button', { name: 'Stop' });
    expect(button.getAttribute('title')).toMatch(/Interrupt Claude, like pressing Escape/);
    fireEvent.click(button);
    await waitFor(() => expect(stopped).toBe(SESSION_ID));
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
  });

  it('hides the control while a dispatched send still awaits its echo', async () => {
    // The echo has not arrived: that row's own Cancel is the way out, and a
    // stop would race the echo.
    serveSends([serverSend({ id: 8, text: 'typed, not echoed', status: 'dispatched' })], {
      state: 'awaiting_echo',
      send_id: 8,
      thread_id: 1,
    });

    renderStrip(surface, undefined, false, true);

    await screen.findByText('typed, not echoed');
    expect(stopButton()).toBeNull();
  });

  it('offers the control for a dispatched send whose echo already arrived', async () => {
    // The echo arrived (the turn is in flight) but the send's transcript line
    // has not been ingested, so it is still `dispatched`. Its Cancel is refused
    // as `send_not_cancellable` there, so Stop is the way out.
    serveSends([serverSend({ id: 8, text: 'echoed, not matched', status: 'dispatched' })], {
      state: 'in_flight',
      send_id: 8,
      thread_id: 1,
    });

    renderStrip(surface, undefined, false, true);

    await screen.findByRole('button', { name: 'Stop' });
  });

  it('points a refused cancel of an echoed send at Stop', async () => {
    // The row's Cancel is still shown, but the server refuses it once the
    // echo arrived; the notice has to name the control that does work.
    serveSends([serverSend({ id: 8, text: 'echoed, not matched', status: 'dispatched' })], {
      state: 'in_flight',
      send_id: 8,
      thread_id: 1,
    });
    server.use(
      http.post('*/api/sends/:id/cancel', () =>
        HttpResponse.json(
          { error: 'send 8 is not cancellable', code: 'send_not_cancellable' },
          { status: 409 },
        ),
      ),
    );

    renderStrip(surface, undefined, false, true);

    await screen.findByRole('button', { name: 'Stop' });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(useNotificationStore.getState().notifications).toHaveLength(1);
    });
    const [notice] = useNotificationStore.getState().notifications;
    expect(notice.detail).toMatch(/Use Stop to interrupt Claude/);
  });

  it('hides the control when no turn is running', async () => {
    serveSends([serverSend({ id: 8, text: 'queued while idle', status: 'queued' })], {
      state: 'idle',
      send_id: null,
      thread_id: null,
    });

    renderStrip(surface, undefined, false, true);

    await screen.findByText('queued while idle');
    expect(stopButton()).toBeNull();
  });

  it('hides the control for a terminal-less provider', async () => {
    serveStuckTurn();

    renderStrip(surface, undefined, false, false);

    await screen.findByText('waiting behind');
    await waitFor(() =>
      expect(useLiveStore.getState().runningThreads[SESSION_ID]).toBeDefined(),
    );
    expect(stopButton()).toBeNull();
  });

  it('surfaces a refused stop through the notification store', async () => {
    serveStuckTurn();
    server.use(
      http.post('*/api/sessions/:id/stop-turn', () =>
        HttpResponse.json(
          {
            error: `session ${SESSION_ID} has no turn to stop`,
            code: 'turn_not_stoppable',
          },
          { status: 409 },
        ),
      ),
    );

    renderStrip(surface, undefined, false, true);

    fireEvent.click(await screen.findByRole('button', { name: 'Stop' }));
    await waitFor(() => {
      expect(useNotificationStore.getState().notifications).toHaveLength(1);
    });
    const [notice] = useNotificationStore.getState().notifications;
    expect(notice.title).toBe('Could not stop the turn');
    expect(notice.detail).toMatch(/no longer in progress/);
  });
});
