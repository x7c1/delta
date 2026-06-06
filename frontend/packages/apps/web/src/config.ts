/** Whether the app should run against MSW + the WS fake (no backend). */
export function isMockMode(): boolean {
  return import.meta.env.VITE_API_MOCK === '1';
}

/** Base URL for REST calls. Empty string means same-origin relative paths. */
export function apiBaseUrl(): string {
  return import.meta.env.VITE_API_BASE_URL ?? '';
}

/** Full ws:// URL of the live `/ws` event stream for the current origin. */
export function wsUrl(path: '/ws' | '/pty'): string {
  const base = apiBaseUrl();
  if (base) {
    return base.replace(/^http/, 'ws').replace(/\/$/, '') + path;
  }
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}${path}`;
}
