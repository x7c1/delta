import type { Page } from '@playwright/test';

/**
 * Live-socket fault injection for the fake-mode suite.
 *
 * The app's `/ws` event stream reconnects on its own after a dropped socket
 * (`WsEventSource`), and every re-open triggers a full REST resync. These
 * helpers force that disconnect/reconnect cycle from the page: an init script
 * wraps `window.WebSocket` so a spec can close the live event socket and
 * temporarily refuse new `/ws` connections (each refused attempt fails like a
 * dead server, exercising the client's real backoff), while every other
 * socket on the page — the PTY bridge, Vite's HMR channel — passes through
 * untouched.
 *
 * Closing from the page is deliberately equivalent to a server-side drop: a
 * client cannot tell who closed a socket, and `WsEventSource` handles every
 * close identically, so this exercises the exact production reconnect path
 * without adding any test-only server surface.
 */

/** The in-page registry installed by {@link interceptLiveSocket}. */
interface LiveSocketControl {
  /** While true, a new `/ws` connection fails like a refused connection. */
  blocked: boolean;
  /** Every real `/ws` socket the page opened, oldest first. */
  sockets: WebSocket[];
  /** How many times a real `/ws` socket reached `open`. */
  opens: number;
}

const CONTROL_KEY = '__deltaE2eLiveSocketControl';

/**
 * Install the `/ws` interception. Must be called before the first `goto`
 * (init scripts run before any page script, so the app's own socket is
 * already created through the wrapper).
 */
export async function interceptLiveSocket(page: Page): Promise<void> {
  await page.addInitScript((key: string) => {
    const control: LiveSocketControl = { blocked: false, sockets: [], opens: 0 };
    (window as unknown as Record<string, LiveSocketControl>)[key] = control;

    const RealWebSocket = window.WebSocket;
    const isLiveEventUrl = (url: string | URL): boolean =>
      new URL(String(url), window.location.href).pathname === '/ws';

    // A socket that behaves like a refused connection: it never opens and
    // reports `close` on the next tick — the same observable sequence a real
    // socket gives when the server is unreachable, which is what the client's
    // reconnect backoff handles.
    const refusedSocket = (): WebSocket => {
      const closeListeners: Array<() => void> = [];
      const stub = {
        readyState: RealWebSocket.CONNECTING as number,
        addEventListener(type: string, listener: () => void): void {
          if (type === 'close') {
            closeListeners.push(listener);
          }
        },
        removeEventListener(): void {
          // Nothing to remove: the stub dies on the next tick.
        },
        send(): void {
          // A never-opened socket accepts no frames; the client sends none.
        },
        close(): void {
          // Already closing; the scheduled close event still fires once.
        },
      };
      setTimeout(() => {
        stub.readyState = RealWebSocket.CLOSED;
        for (const listener of closeListeners) {
          listener();
        }
      }, 0);
      return stub as unknown as WebSocket;
    };

    function ControlledWebSocket(
      url: string | URL,
      protocols?: string | string[],
    ): WebSocket {
      if (!isLiveEventUrl(url)) {
        return new RealWebSocket(url, protocols);
      }
      if (control.blocked) {
        return refusedSocket();
      }
      const socket = new RealWebSocket(url, protocols);
      control.sockets.push(socket);
      socket.addEventListener('open', () => {
        control.opens += 1;
      });
      return socket;
    }
    ControlledWebSocket.prototype = RealWebSocket.prototype;
    Object.assign(ControlledWebSocket, {
      CONNECTING: RealWebSocket.CONNECTING,
      OPEN: RealWebSocket.OPEN,
      CLOSING: RealWebSocket.CLOSING,
      CLOSED: RealWebSocket.CLOSED,
    });
    window.WebSocket = ControlledWebSocket as unknown as typeof WebSocket;
  }, CONTROL_KEY);
}

/**
 * Close the live event socket and refuse reconnect attempts until
 * {@link restoreLiveSocket}. Events the server broadcasts in the meantime are
 * lost (the server does not replay), exactly as in a real outage.
 */
export async function dropLiveSocket(page: Page): Promise<void> {
  await page.evaluate((key: string) => {
    const control = (
      window as unknown as Record<string, LiveSocketControl | undefined>
    )[key];
    if (!control) {
      throw new Error('interceptLiveSocket(page) must run before page.goto');
    }
    control.blocked = true;
    for (const socket of control.sockets) {
      if (
        socket.readyState === WebSocket.CONNECTING ||
        socket.readyState === WebSocket.OPEN
      ) {
        socket.close();
      }
    }
  }, CONTROL_KEY);
}

/**
 * Lift the block and wait until the client's own backoff lands a real `/ws`
 * connection again. The wait is generous because it spans whatever backoff
 * step the client reached while blocked (500 ms growing to seconds) — this is
 * the production reconnect cadence, not a test-tunable delay.
 */
export async function restoreLiveSocket(page: Page): Promise<void> {
  const opensBefore = await page.evaluate((key: string) => {
    const control = (
      window as unknown as Record<string, LiveSocketControl | undefined>
    )[key];
    if (!control) {
      throw new Error('interceptLiveSocket(page) must run before page.goto');
    }
    control.blocked = false;
    return control.opens;
  }, CONTROL_KEY);
  await page.waitForFunction(
    ([key, before]) => {
      const control = (
        window as unknown as Record<string, LiveSocketControl | undefined>
      )[key as string];
      return (control?.opens ?? 0) > (before as number);
    },
    [CONTROL_KEY, opensBefore] as const,
    { timeout: 15_000 },
  );
}
