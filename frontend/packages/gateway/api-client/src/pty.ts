/**
 * The `/pty` terminal bridge client. Confines the raw binary WebSocket here so
 * the UI layer only deals with byte callbacks. Server frames are binary PTY
 * output; browser frames are input bytes written into the PTY.
 */

export interface PtyConnectionOptions {
  url: string;
  /** Called with each chunk of PTY output. */
  onData: (chunk: Uint8Array) => void;
  /** Called when the socket opens / closes. */
  onStatus?: (status: 'open' | 'closed') => void;
  socketFactory?: (url: string) => WebSocket;
}

export interface PtyConnection {
  /** Write input bytes into the PTY. */
  send(data: string | Uint8Array): void;
  close(): void;
}

export function connectPty(options: PtyConnectionOptions): PtyConnection {
  const factory =
    options.socketFactory ?? ((url: string) => new WebSocket(url));
  const socket = factory(options.url);
  socket.binaryType = 'arraybuffer';

  socket.addEventListener('open', () => options.onStatus?.('open'));
  socket.addEventListener('close', () => options.onStatus?.('closed'));
  socket.addEventListener('message', (event: MessageEvent) => {
    if (event.data instanceof ArrayBuffer) {
      options.onData(new Uint8Array(event.data));
    } else if (typeof event.data === 'string') {
      options.onData(new TextEncoder().encode(event.data));
    }
  });

  return {
    send(data) {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(data);
      }
    },
    close() {
      socket.close();
    },
  };
}
