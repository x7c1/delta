/**
 * Where the page keeps the server's per-run bearer token once it has traded a
 * one-time code for it (see `establishAuth.ts`).
 *
 * `localStorage` is scoped to the page's origin, port included, so two Delta
 * servers on different ports never share a token, and a reload or a second
 * tab of the same server finds it without a new code. A cookie is deliberately
 * not used: cookies are not scoped by port, so one set by this server would be
 * sent to every other listener on the same loopback host.
 */

/** The `localStorage` key the token is kept under. */
export const AUTH_TOKEN_STORAGE_KEY = 'delta.auth.token';

/**
 * The stored token, or `''` when there is none. Storage that cannot be read
 * (a browser that blocks it) also reads as none, which shows the screen that
 * asks for a new link.
 */
export function storedAuthToken(): string {
  try {
    return window.localStorage.getItem(AUTH_TOKEN_STORAGE_KEY) ?? '';
  } catch {
    return '';
  }
}

/**
 * Keep `token` for this origin. Throws when storage cannot be written, so the
 * caller can say that the browser refuses to keep it.
 */
export function storeAuthToken(token: string): void {
  window.localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, token);
}
