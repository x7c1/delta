import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  parseSessionEvent,
  WsEventSource,
  type ConnectionStatus,
} from './ws';

/**
 * A hand-driven WebSocket stand-in: tests dispatch `open`/`close`/`message`
 * explicitly and inspect how many sockets the source created (one per
 * connection attempt) and whether it was asked to close.
 */
class FakeSocket {
  private readonly listeners: Record<string, ((ev: unknown) => void)[]> = {};
  closeCalls = 0;
  addEventListener(type: string, cb: (ev: unknown) => void): void {
    (this.listeners[type] ??= []).push(cb);
  }
  dispatch(type: string, ev?: unknown): void {
    (this.listeners[type] ?? []).forEach((cb) => cb(ev));
  }
  close(): void {
    this.closeCalls += 1;
  }
}

describe('parseSessionEvent', () => {
  it('parses a known event kind', () => {
    const event = parseSessionEvent(
      JSON.stringify({
        kind: 'turn_started',
        session_id: 'sess-1',
        pending_send_id: 1,
        matched_uuid: 'uuid-1',
      }),
    );

    expect(event).toEqual({
      kind: 'turn_started',
      session_id: 'sess-1',
      pending_send_id: 1,
      matched_uuid: 'uuid-1',
    });
  });

  it('parses a transcript_updated event', () => {
    const event = parseSessionEvent(
      JSON.stringify({
        kind: 'transcript_updated',
        session_id: 'sess-1',
        thread_ids: [1, 4],
      }),
    );

    expect(event).toEqual({
      kind: 'transcript_updated',
      session_id: 'sess-1',
      thread_ids: [1, 4],
    });
  });

  it('parses the session_opened and session_closed lifecycle events', () => {
    expect(
      parseSessionEvent(
        JSON.stringify({ kind: 'session_opened', session_id: 'sess-1' }),
      ),
    ).toEqual({ kind: 'session_opened', session_id: 'sess-1' });

    expect(
      parseSessionEvent(
        JSON.stringify({ kind: 'session_closed', session_id: 'sess-1' }),
      ),
    ).toEqual({ kind: 'session_closed', session_id: 'sess-1' });
  });

  it('returns null for an unknown kind', () => {
    expect(parseSessionEvent(JSON.stringify({ kind: 'bogus' }))).toBeNull();
  });

  it('returns null for malformed JSON', () => {
    expect(parseSessionEvent('not json')).toBeNull();
  });
});

describe('WsEventSource reconnection', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('reconnects after an unexpected close and re-emits open', () => {
    vi.useFakeTimers();
    const sockets: FakeSocket[] = [];
    const source = new WsEventSource({
      url: 'ws://localhost/ws',
      socketFactory: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
      reconnectDelaysMs: [100],
    });
    const statuses: ConnectionStatus[] = [];
    source.onStatus((status) => statuses.push(status));

    // First connection is live, then the socket drops unexpectedly.
    expect(sockets).toHaveLength(1);
    sockets[0].dispatch('open');
    sockets[0].dispatch('close');

    // A reconnect is scheduled, not immediate.
    expect(sockets).toHaveLength(1);
    vi.advanceTimersByTime(100);
    expect(sockets).toHaveLength(2);

    // The fresh socket opening re-emits `open`, so live updates resume.
    sockets[1].dispatch('open');
    expect(statuses).toEqual(['open', 'closed', 'connecting', 'open']);
  });

  it('does not reconnect after an explicit close', () => {
    vi.useFakeTimers();
    const sockets: FakeSocket[] = [];
    const source = new WsEventSource({
      url: 'ws://localhost/ws',
      socketFactory: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
      reconnectDelaysMs: [100],
    });

    sockets[0].dispatch('open');
    source.close();
    // The real socket then fires its close event; it must not trigger a retry.
    sockets[0].dispatch('close');

    expect(sockets[0].closeCalls).toBe(1);
    vi.advanceTimersByTime(1000);
    expect(sockets).toHaveLength(1);
  });
});
