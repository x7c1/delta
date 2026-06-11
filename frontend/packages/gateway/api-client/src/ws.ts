import { EVENT_KINDS, type SessionEvent } from '@delta/wire-gen';

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

/**
 * The recognised `kind` discriminants, generated from the backend's wire
 * contract. Frames whose `kind` is not in this set (an older or newer backend)
 * are dropped without throwing.
 */
const KNOWN_KINDS: ReadonlySet<string> = new Set(EVENT_KINDS);

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
    KNOWN_KINDS.has((parsed as { kind: string }).kind)
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

/**
 * Reconnect backoff schedule (ms). The server broadcast channel does not replay
 * missed events, so a dropped socket must reconnect quickly; the delay grows on
 * repeated failures and then holds at the last value.
 */
const DEFAULT_RECONNECT_DELAYS_MS = [500, 1000, 2000, 5000, 10000];

/**
 * `WebSocket.CONNECTING` (readyState `0`). Spelled as a literal so this module
 * does not depend on a global `WebSocket`, which is absent in the non-DOM test
 * environment (and would also misfire against the hand-rolled fake socket).
 */
const WS_CONNECTING = 0;

export interface WsClientOptions {
  /** Full ws(s) URL of the `/ws` endpoint. */
  url: string;
  /** Injectable WebSocket constructor, primarily for tests. */
  socketFactory?: (url: string) => WebSocket;
  /** Backoff schedule (ms) for reconnection; the last value repeats. */
  reconnectDelaysMs?: number[];
}

/**
 * A live `SessionEventSource` backed by a real WebSocket connection that
 * **reconnects automatically**.
 *
 * The `/ws` stream is the only channel that drains the optimistic pending-send
 * FIFO (via `turn_completed`) and refreshes the transcript. Without
 * reconnection a single dropped socket — a server hiccup, a dev-proxy blip, an
 * idle timeout — would silently freeze all live updates until a full page
 * reload: pending sends would stick on "waiting" forever and the transcript
 * would stop growing. So on an unexpected close this source schedules a backoff
 * reconnect and re-emits `connecting` → `open`, letting the app resync the gap
 * on each fresh `open`. An explicit {@link close} (component unmount) suppresses
 * reconnection.
 */
export class WsEventSource implements SessionEventSource {
  private readonly emitter = new EventEmitter();
  private readonly factory: (url: string) => WebSocket;
  private readonly url: string;
  private readonly reconnectDelaysMs: number[];
  private socket: WebSocket | null = null;
  private closedByClient = false;
  private attempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(options: WsClientOptions) {
    this.factory =
      options.socketFactory ?? ((url: string) => new WebSocket(url));
    this.url = options.url;
    this.reconnectDelaysMs =
      options.reconnectDelaysMs ?? DEFAULT_RECONNECT_DELAYS_MS;
    this.connect();
  }

  private connect(): void {
    this.emitter.emitStatus('connecting');
    const socket = this.factory(this.url);
    this.socket = socket;
    socket.addEventListener('open', () => {
      this.attempt = 0;
      this.emitter.emitStatus('open');
    });
    socket.addEventListener('close', () => {
      this.handleDisconnect();
    });
    socket.addEventListener('message', (message: MessageEvent) => {
      if (typeof message.data !== 'string') {
        return;
      }
      const event = parseSessionEvent(message.data);
      if (event) {
        this.emitter.emitEvent(event);
      }
    });
  }

  private handleDisconnect(): void {
    this.emitter.emitStatus('closed');
    if (this.closedByClient) {
      return;
    }
    // Reconnect after a backoff. Events broadcast while we are disconnected are
    // gone (the server does not replay), so the app resyncs on the next `open`.
    const delay =
      this.reconnectDelaysMs[
        Math.min(this.attempt, this.reconnectDelaysMs.length - 1)
      ];
    this.attempt += 1;
    this.reconnectTimer = setTimeout(() => this.connect(), delay);
  }

  onEvent(listener: SessionEventListener): () => void {
    return this.emitter.onEvent(listener);
  }

  onStatus(listener: ConnectionStatusListener): () => void {
    return this.emitter.onStatus(listener);
  }

  close(): void {
    this.closedByClient = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    const socket = this.socket;
    if (socket === null) {
      return;
    }
    if (socket.readyState === WS_CONNECTING) {
      // Closing a socket mid-handshake makes browsers log "WebSocket is closed
      // before the connection is established". Defer the close until the
      // handshake settles, then close cleanly. This is routine under React
      // StrictMode's dev-only mount → unmount → mount, which tears down the
      // first socket while it is still connecting.
      socket.addEventListener('open', () => socket.close(), { once: true });
    } else {
      socket.close();
    }
  }
}
