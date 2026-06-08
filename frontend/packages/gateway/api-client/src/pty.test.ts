import { describe, expect, it } from 'vitest';
import { connectPty } from './pty';
import type { SessionId } from '@delta/model';

/** A minimal fake WebSocket capturing sends and exposing a settable state. */
class FakeWebSocket {
  static readonly OPEN = 1;
  static readonly CLOSED = 3;
  binaryType = 'blob';
  readyState = FakeWebSocket.OPEN;
  readonly sent: Array<string | ArrayBufferView | ArrayBuffer> = [];

  constructor(public readonly url: string) {}

  addEventListener(): void {}
  send(data: string | ArrayBufferView | ArrayBuffer): void {
    this.sent.push(data);
  }
  close(): void {
    this.readyState = FakeWebSocket.CLOSED;
  }
}

function connect(socket: FakeWebSocket) {
  return connectPty({
    url: 'ws://test/pty',
    sessionId: 'sess-1' as SessionId,
    onData: () => {},
    socketFactory: () => socket as unknown as WebSocket,
  });
}

describe('connectPty', () => {
  it('sends string input as UTF-8 binary bytes', () => {
    const socket = new FakeWebSocket('ws://test/pty');
    const conn = connect(socket);

    conn.send('hi');

    expect(socket.sent).toHaveLength(1);
    const frame = socket.sent[0];
    expect(typeof frame).not.toBe('string');
    expect(frame).toEqual(new TextEncoder().encode('hi'));
  });

  it('sends Uint8Array input as-is (binary)', () => {
    const socket = new FakeWebSocket('ws://test/pty');
    const conn = connect(socket);
    const bytes = new Uint8Array([1, 2, 3]);

    conn.send(bytes);

    expect(socket.sent).toEqual([bytes]);
  });

  it('sends resize as a JSON text control frame', () => {
    const socket = new FakeWebSocket('ws://test/pty');
    const conn = connect(socket);

    conn.resize(40, 120);

    expect(socket.sent).toEqual([
      JSON.stringify({ type: 'resize', rows: 40, cols: 120 }),
    ]);
  });

  it('suppresses sends and resizes when the socket is not open', () => {
    const socket = new FakeWebSocket('ws://test/pty');
    socket.readyState = FakeWebSocket.CLOSED;
    const conn = connect(socket);

    conn.send('hi');
    conn.send(new Uint8Array([1]));
    conn.resize(10, 20);

    expect(socket.sent).toEqual([]);
  });
});
