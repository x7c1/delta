import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SessionId } from '@delta/model';
import { ApiClient } from './http';
import { WsEventSource } from './ws';
import { connectCommsLog } from './comms';
import { connectPty } from './pty';

/**
 * A refused token shows up on a live socket only as a close, so each socket
 * client reports a close the app did not ask for, and the app hands that to
 * `ApiClient.recheckAuth`. These tests wire the three clients to a real
 * `ApiClient` over a fake `fetch`, exactly as the app does.
 */

/** A hand-driven WebSocket stand-in: tests dispatch `open` / `close`. */
class FakeSocket {
  static readonly OPEN = 1;
  readyState = 0;
  binaryType = 'blob';
  closeCalls = 0;
  private readonly listeners: Record<string, ((ev: unknown) => void)[]> = {};

  addEventListener(type: string, cb: (ev: unknown) => void): void {
    (this.listeners[type] ??= []).push(cb);
  }

  dispatch(type: 'open' | 'close'): void {
    this.readyState = type === 'open' ? 1 : 3;
    (this.listeners[type] ?? []).forEach((cb) => cb({}));
  }

  /** Like a browser: the `close` event follows the app's own close. */
  close(): void {
    this.closeCalls += 1;
    this.dispatch('close');
  }
}

/** What the REST re-check answers. */
type Recheck = 'refused' | 'accepted' | 'network-down';

function fetchAnswering(recheck: Recheck) {
  return vi.fn(async () => {
    switch (recheck) {
      case 'refused':
        return new Response(null, { status: 401 });
      case 'accepted':
        return new Response(JSON.stringify({ version: 'v0.0.0' }), {
          status: 200,
        });
      case 'network-down':
        throw new TypeError('Failed to fetch');
    }
  });
}

/** One socket client under test, opened against the given socket stand-ins. */
interface SocketClient {
  name: string;
  open(
    factory: () => WebSocket,
    onUnexpectedClose: () => void,
    onStatus: (status: string) => void,
  ): { close(): void };
}

const clients: SocketClient[] = [
  {
    name: 'event (/ws)',
    open: (factory, onUnexpectedClose, onStatus) => {
      const source = new WsEventSource({
        url: 'ws://test/ws',
        socketFactory: factory,
        reconnectDelaysMs: [10],
        onUnexpectedClose,
      });
      source.onStatus(onStatus);
      return source;
    },
  },
  {
    name: 'comms (/comms)',
    open: (factory, onUnexpectedClose, onStatus) =>
      connectCommsLog({
        url: 'ws://test/comms',
        sessionId: 'sess-1' as SessionId,
        onFrame: () => {},
        onStatus,
        socketFactory: factory,
        onUnexpectedClose,
      }),
  },
  {
    name: 'pty (/pty)',
    open: (factory, onUnexpectedClose, onStatus) =>
      connectPty({
        url: 'ws://test/pty',
        sessionId: 'sess-1' as SessionId,
        onData: () => {},
        onStatus,
        socketFactory: factory,
        onUnexpectedClose,
      }),
  },
];

/** Let the re-check's fetch and its handlers settle. */
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) {
    await Promise.resolve();
  }
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(console, 'debug').mockImplementation(() => {});
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe.each(clients)('the $name socket', (client) => {
  function setUp(recheck: Recheck) {
    const sockets: FakeSocket[] = [];
    const factory = () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    };
    const fetchFn = fetchAnswering(recheck);
    const onUnauthorized = vi.fn();
    const api = new ApiClient({ fetchFn, token: 'tok', onUnauthorized });
    const statuses: string[] = [];
    const connection = client.open(
      factory,
      () => void api.recheckAuth(),
      (status) => statuses.push(status),
    );
    return { sockets, fetchFn, onUnauthorized, statuses, connection };
  }

  it('reports a refused token once when it closes unexpectedly', async () => {
    const { sockets, fetchFn, onUnauthorized } = setUp('refused');
    sockets[0].dispatch('open');

    sockets[0].dispatch('close');
    await settle();

    expect(fetchFn).toHaveBeenCalledTimes(1);
    const [url, init] = fetchFn.mock.calls[0] as unknown as [
      string,
      RequestInit,
    ];
    expect(url).toBe('/api/version');
    expect(new Headers(init.headers).get('Authorization')).toBe('Bearer tok');
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });

  it.each<Recheck>(['accepted', 'network-down'])(
    'leaves the gate alone when the re-check is %s',
    async (recheck) => {
      const { sockets, fetchFn, onUnauthorized, statuses } = setUp(recheck);
      sockets[0].dispatch('open');

      sockets[0].dispatch('close');
      await settle();

      expect(fetchFn).toHaveBeenCalledTimes(1);
      expect(onUnauthorized).not.toHaveBeenCalled();
      expect(statuses).toContain('closed');
    },
  );

  it('does not re-check a socket the app closed itself', async () => {
    const { sockets, fetchFn, connection } = setUp('refused');
    sockets[0].dispatch('open');

    connection.close();
    await settle();
    await vi.runAllTimersAsync();

    expect(sockets[0].closeCalls).toBe(1);
    expect(fetchFn).not.toHaveBeenCalled();
  });
});

describe('the event socket after an unexpected close', () => {
  it.each<Recheck>(['accepted', 'network-down'])(
    'reconnects as before when the re-check is %s',
    async (recheck) => {
      const sockets: FakeSocket[] = [];
      const fetchFn = fetchAnswering(recheck);
      const onUnauthorized = vi.fn();
      const api = new ApiClient({ fetchFn, token: 'tok', onUnauthorized });
      const source = new WsEventSource({
        url: 'ws://test/ws',
        socketFactory: () => {
          const socket = new FakeSocket();
          sockets.push(socket);
          return socket as unknown as WebSocket;
        },
        reconnectDelaysMs: [10],
        onUnexpectedClose: () => void api.recheckAuth(),
      });
      sockets[0].dispatch('open');

      sockets[0].dispatch('close');
      await vi.advanceTimersByTimeAsync(10);

      expect(sockets).toHaveLength(2);
      expect(onUnauthorized).not.toHaveBeenCalled();

      // Each close is re-checked once, never polled in between.
      await vi.advanceTimersByTimeAsync(1000);
      expect(fetchFn).toHaveBeenCalledTimes(1);
      source.close();
    },
  );
});
