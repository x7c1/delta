import { storedAuthToken } from './features/auth/tokenStore';

/** Whether the app should run against MSW + the WS fake (no backend). */
export function isMockMode(): boolean {
  return import.meta.env.VITE_API_MOCK === '1';
}

/** Base URL for REST calls. Empty string means same-origin relative paths. */
export function apiBaseUrl(): string {
  return import.meta.env.VITE_API_BASE_URL ?? '';
}

/**
 * The per-run bearer token the server requires on every API call and live
 * socket: the one this page traded a one-time code for and keeps in
 * `localStorage` (see `features/auth/`). No page the server serves carries
 * it. Empty in mock mode, where no real backend is reached, and before the
 * page holds one — callers treat an empty token as "attach nothing".
 */
export function authToken(): string {
  if (isMockMode() || typeof window === 'undefined') {
    return '';
  }
  return storedAuthToken();
}

/**
 * Full ws:// URL of one of the server's WebSocket endpoints for the current
 * origin: the `/ws` event stream, the `/pty` terminal bridge, or the `/comms`
 * observability log.
 *
 * When a per-run token is present it rides as a `token=` query parameter,
 * because a browser cannot set headers on a WebSocket upgrade. Downstream
 * `?session_id=` joiners use `url.includes('?') ? '&' : '?'`, so appending it
 * here is safe regardless of order. In mock mode the token is empty and nothing
 * is appended.
 */
export function wsUrl(path: '/ws' | '/pty' | '/comms'): string {
  const base = apiBaseUrl();
  const raw = base
    ? base.replace(/^http/, 'ws').replace(/\/$/, '') + path
    : `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${
        window.location.host
      }${path}`;
  const token = authToken();
  if (!token) {
    return raw;
  }
  const separator = raw.includes('?') ? '&' : '?';
  return `${raw}${separator}token=${encodeURIComponent(token)}`;
}
