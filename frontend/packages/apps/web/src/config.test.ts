import { afterEach, describe, expect, it } from 'vitest';
import { authToken, wsUrl } from './config';
import { AUTH_TOKEN_STORAGE_KEY } from './features/auth/tokenStore';

/** Store (or clear) the token the page traded its one-time code for. */
function setStoredToken(token: string | null): void {
  if (token === null) {
    window.localStorage.removeItem(AUTH_TOKEN_STORAGE_KEY);
  } else {
    window.localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, token);
  }
}

afterEach(() => {
  setStoredToken(null);
});

describe('authToken', () => {
  it('reads the token the page keeps in localStorage', () => {
    setStoredToken('secret-token');
    expect(authToken()).toBe('secret-token');
  });

  it('is empty when the page holds no token', () => {
    setStoredToken(null);
    expect(authToken()).toBe('');
  });
});

describe('wsUrl', () => {
  it('appends token= when a token is present', () => {
    setStoredToken('secret-token');
    const url = wsUrl('/ws');
    expect(url).toMatch(/\/ws\?token=secret-token$/);
    expect(url.startsWith('ws')).toBe(true);
  });

  it('percent-encodes the token', () => {
    setStoredToken('a/b c');
    expect(wsUrl('/pty')).toMatch(/\/pty\?token=a%2Fb%20c$/);
  });

  it('omits the token when none is present', () => {
    setStoredToken(null);
    const url = wsUrl('/comms');
    expect(url).toMatch(/\/comms$/);
    expect(url).not.toContain('token=');
  });
});
