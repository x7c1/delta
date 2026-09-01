import { afterEach, describe, expect, it } from 'vitest';
import { authToken, wsUrl } from './config';

/** Set (or clear) the `delta-auth-token` meta tag the token is read from. */
function setTokenMeta(token: string | null): void {
  document.querySelector('meta[name="delta-auth-token"]')?.remove();
  if (token !== null) {
    const meta = document.createElement('meta');
    meta.name = 'delta-auth-token';
    meta.content = token;
    document.head.appendChild(meta);
  }
}

afterEach(() => {
  setTokenMeta(null);
});

describe('authToken', () => {
  it('reads the token from the meta tag', () => {
    setTokenMeta('secret-token');
    expect(authToken()).toBe('secret-token');
  });

  it('is empty when the meta tag is absent (mock mode)', () => {
    setTokenMeta(null);
    expect(authToken()).toBe('');
  });
});

describe('wsUrl', () => {
  it('appends token= when a token is present', () => {
    setTokenMeta('secret-token');
    const url = wsUrl('/ws');
    expect(url).toMatch(/\/ws\?token=secret-token$/);
    expect(url.startsWith('ws')).toBe(true);
  });

  it('percent-encodes the token', () => {
    setTokenMeta('a/b c');
    expect(wsUrl('/pty')).toMatch(/\/pty\?token=a%2Fb%20c$/);
  });

  it('omits the token when none is present (mock mode)', () => {
    setTokenMeta(null);
    const url = wsUrl('/comms');
    expect(url).toMatch(/\/comms$/);
    expect(url).not.toContain('token=');
  });
});
