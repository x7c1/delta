import { describe, expect, it, vi } from 'vitest';
import type { CommsFrame } from '@delta/wire-gen';
import type { SessionId } from '@delta/model';
import {
  connectCommsLog,
  parseCommsFrame,
  type CommsLogConnection,
  type CommsLogOptions,
} from './comms';

/**
 * A minimal fake WebSocket that records its listeners so a test can deliver
 * server frames synchronously.
 */
class FakeWebSocket {
  closed = false;
  private readonly listeners = new Map<string, ((event: unknown) => void)[]>();

  constructor(public readonly url: string) {}

  addEventListener(type: string, listener: (event: unknown) => void): void {
    const existing = this.listeners.get(type) ?? [];
    existing.push(listener);
    this.listeners.set(type, existing);
  }

  close(): void {
    this.closed = true;
    this.fire('close', {});
  }

  /** Deliver one server message. */
  message(data: unknown): void {
    this.fire('message', { data });
  }

  open(): void {
    this.fire('open', {});
  }

  private fire(type: string, event: unknown): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

/** One well-formed frame, as the server would send it. */
function frame(overrides: Partial<CommsFrame> = {}): CommsFrame {
  return {
    seq: 0,
    at_ms: 1_760_000_000_000,
    direction: 'to_agent',
    kind: 'request',
    method: 'turn/start',
    payload_json: '{"id":1,"method":"turn/start"}',
    ...overrides,
  };
}

/**
 * Connect over a {@link FakeWebSocket}, returning it alongside the connection so
 * a test can deliver server frames. Defaults stand in for whatever the test does
 * not care about.
 */
function connect(options: Partial<CommsLogOptions> = {}): {
  socket: FakeWebSocket;
  connection: CommsLogConnection;
} {
  let socket!: FakeWebSocket;
  const connection = connectCommsLog({
    url: options.url ?? 'ws://test/comms',
    sessionId: options.sessionId ?? ('sess-1' as SessionId),
    onFrame: options.onFrame ?? (() => {}),
    onStatus: options.onStatus,
    onError: options.onError,
    socketFactory: (url) => {
      socket = new FakeWebSocket(url);
      return socket as unknown as WebSocket;
    },
  });
  return { socket, connection };
}

describe('connectCommsLog', () => {
  it('names the session in the query string, so the server knows whose log to send', () => {
    const { socket } = connect({ sessionId: 'sess/1' as SessionId });
    // The id is percent-encoded: a session id is opaque and may contain
    // characters a query string reserves.
    expect(socket.url).toBe('ws://test/comms?session_id=sess%2F1');
  });

  it('reports each server frame in arrival order', () => {
    const received: CommsFrame[] = [];
    const { socket } = connect({ onFrame: (one) => received.push(one) });

    socket.message(JSON.stringify(frame({ seq: 0 })));
    socket.message(JSON.stringify(frame({ seq: 1, method: 'turn/completed' })));

    expect(received.map((f) => f.seq)).toEqual([0, 1]);
    expect(received[1].method).toBe('turn/completed');
  });

  it('reports open and close through onStatus', () => {
    const statuses: string[] = [];
    const { socket } = connect({ onStatus: (status) => statuses.push(status) });

    socket.open();
    socket.close();

    expect(statuses).toEqual(['open', 'closed']);
  });

  it('skips an unreadable frame and keeps streaming the rest', () => {
    // The log is observability: one frame the browser cannot read must never
    // cost the reader the frames around it.
    const received: CommsFrame[] = [];
    const errors: unknown[] = [];
    const { socket } = connect({
      onFrame: (one) => received.push(one),
      onError: (error) => errors.push(error),
    });

    socket.message('not json at all');
    socket.message(JSON.stringify({ unexpected: 'shape' }));
    socket.message(JSON.stringify(frame({ seq: 7 })));

    expect(errors).toHaveLength(2);
    expect(received.map((f) => f.seq)).toEqual([7]);
  });

  it('ignores a binary frame: the stream is JSON text only', () => {
    const onFrame = vi.fn();
    const { socket } = connect({ onFrame });

    socket.message(new ArrayBuffer(4));

    expect(onFrame).not.toHaveBeenCalled();
  });

  it('closes the underlying socket', () => {
    const { socket, connection } = connect();

    connection.close();

    expect(socket.closed).toBe(true);
  });
});

describe('parseCommsFrame', () => {
  it('accepts a well-formed frame', () => {
    expect(parseCommsFrame(JSON.stringify(frame()))?.method).toBe('turn/start');
  });

  it('rejects a payload that is not a frame object', () => {
    // The two fields the UI cannot render without: the ordering key and the
    // payload text.
    expect(parseCommsFrame('42')).toBeNull();
    expect(parseCommsFrame('null')).toBeNull();
    expect(parseCommsFrame(JSON.stringify({ seq: 1 }))).toBeNull();
    expect(parseCommsFrame(JSON.stringify({ payload_json: '{}' }))).toBeNull();
  });
});
