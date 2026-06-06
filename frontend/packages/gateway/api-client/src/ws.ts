import type { SessionEvent } from '@delta/model';

/**
 * The WebSocket client for the `/ws` live event stream. This is the only place
 * that touches `WebSocket` directly. It parses incoming text frames into typed
 * `SessionEvent`s and fans them out to subscribers.
 *
 * MSW cannot mock WebSockets, so in mock mode the app substitutes a fake source
 * (see `@delta/api-mocks`) that satisfies the same {@link SessionEventSource}
 * interface.
 */

export type SessionEventListener = (event: SessionEvent) => void;
export type ConnectionStatus = 'connecting' | 'open' | 'closed';
export type ConnectionStatusListener = (status: ConnectionStatus) => void;

export interface SessionEventSource {
  /** Subscribe to parsed session events. Returns an unsubscribe function. */
  onEvent(listener: SessionEventListener): () => void;
  /** Subscribe to connection-status changes. Returns an unsubscribe function. */
  onStatus(listener: ConnectionStatusListener): () => void;
  /** Tear down the underlying transport. */
  close(): void;
}

const EVENT_KINDS: ReadonlySet<string> = new Set([
  'session_registered',
  'turn_started',
  'external_input',
  'turn_completed',
  'permission_requested',
]);

/** Parse a raw text frame into a `SessionEvent`, or `null` if unrecognised. */
export function parseSessionEvent(data: string): SessionEvent | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    return null;
  }
  if (
    typeof parsed === 'object' &&
    parsed !== null &&
    'kind' in parsed &&
    typeof (parsed as { kind: unknown }).kind === 'string' &&
    EVENT_KINDS.has((parsed as { kind: string }).kind)
  ) {
    return parsed as SessionEvent;
  }
  return null;
}

/** Shared fan-out machinery used by both the real and fake event sources. */
export class EventEmitter {
  private readonly eventListeners = new Set<SessionEventListener>();
  private readonly statusListeners = new Set<ConnectionStatusListener>();

  onEvent(listener: SessionEventListener): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onStatus(listener: ConnectionStatusListener): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  emitEvent(event: SessionEvent): void {
    for (const listener of this.eventListeners) {
      listener(event);
    }
  }

  emitStatus(status: ConnectionStatus): void {
    for (const listener of this.statusListeners) {
      listener(status);
    }
  }
}

export interface WsClientOptions {
  /** Full ws(s) URL of the `/ws` endpoint. */
  url: string;
  /** Injectable WebSocket constructor, primarily for tests. */
  socketFactory?: (url: string) => WebSocket;
}

/** A live `SessionEventSource` backed by a real WebSocket connection. */
export class WsEventSource implements SessionEventSource {
  private readonly emitter = new EventEmitter();
  private readonly socket: WebSocket;

  constructor(options: WsClientOptions) {
    const factory =
      options.socketFactory ?? ((url: string) => new WebSocket(url));
    this.emitter.emitStatus('connecting');
    this.socket = factory(options.url);
    this.socket.addEventListener('open', () => {
      this.emitter.emitStatus('open');
    });
    this.socket.addEventListener('close', () => {
      this.emitter.emitStatus('closed');
    });
    this.socket.addEventListener('message', (message: MessageEvent) => {
      if (typeof message.data !== 'string') {
        return;
      }
      const event = parseSessionEvent(message.data);
      if (event) {
        this.emitter.emitEvent(event);
      }
    });
  }

  onEvent(listener: SessionEventListener): () => void {
    return this.emitter.onEvent(listener);
  }

  onStatus(listener: ConnectionStatusListener): () => void {
    return this.emitter.onStatus(listener);
  }

  close(): void {
    this.socket.close();
  }
}
