import type { CommsFrame } from '@delta/wire-gen';
import type { SessionId } from '@delta/model';

/**
 * The `/comms` client: one session's comms log, streamed as JSON text frames.
 *
 * Confines the raw WebSocket here so the UI layer only deals with parsed
 * {@link CommsFrame} objects, mirroring how {@link connectPty} confines the
 * terminal bridge. The stream is one-way — the browser sends nothing — and the
 * server replays its ring buffer before tailing live, so a connection opened
 * mid-session starts with recent history.
 *
 * A malformed frame is reported through {@link CommsLogOptions.onError} and
 * skipped rather than tearing the connection down: the log is observability, and
 * one unreadable frame is not a reason to stop showing the rest.
 */

export interface CommsLogOptions {
  /** Base `/comms` URL; the session id is appended as a query parameter. */
  url: string;
  /** The session whose log to watch. */
  sessionId: SessionId;
  /** Called with each frame, in server order. */
  onFrame: (frame: CommsFrame) => void;
  /** Called when the socket opens / closes. */
  onStatus?: (status: 'open' | 'closed') => void;
  /** Called with a frame that could not be parsed; the stream continues. */
  onError?: (error: unknown) => void;
  socketFactory?: (url: string) => WebSocket;
}

export interface CommsLogConnection {
  close(): void;
}

/** Append the `session_id` query parameter to the base `/comms` URL. */
function withSessionId(url: string, sessionId: SessionId): string {
  const separator = url.includes('?') ? '&' : '?';
  return `${url}${separator}session_id=${encodeURIComponent(sessionId)}`;
}

/**
 * Parse one server text frame.
 *
 * Returns `null` when the text parsed but is not a frame object, and throws when
 * it is not JSON at all. Both are reported the same way by the caller below
 * (through {@link CommsLogOptions.onError}, one frame skipped), so the split only
 * matters to anyone calling this directly.
 */
export function parseCommsFrame(data: string): CommsFrame | null {
  const value: unknown = JSON.parse(data);
  if (
    typeof value !== 'object' ||
    value === null ||
    typeof (value as CommsFrame).seq !== 'number' ||
    typeof (value as CommsFrame).payload_json !== 'string'
  ) {
    return null;
  }
  return value as CommsFrame;
}

export function connectCommsLog(
  options: CommsLogOptions,
): CommsLogConnection {
  const factory =
    options.socketFactory ?? ((url: string) => new WebSocket(url));
  const socket = factory(withSessionId(options.url, options.sessionId));

  socket.addEventListener('open', () => options.onStatus?.('open'));
  socket.addEventListener('close', () => options.onStatus?.('closed'));
  socket.addEventListener('message', (event: MessageEvent) => {
    if (typeof event.data !== 'string') {
      return;
    }
    try {
      const frame = parseCommsFrame(event.data);
      if (frame === null) {
        options.onError?.(new Error('comms frame has an unexpected shape'));
        return;
      }
      options.onFrame(frame);
    } catch (error) {
      options.onError?.(error);
    }
  });

  return {
    close() {
      socket.close();
    },
  };
}
