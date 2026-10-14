import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '@delta/api-client';
import { establishAuth } from './establishAuth';
import { AUTH_TOKEN_STORAGE_KEY, storedAuthToken } from './tokenStore';

/** Put the page at `url` (same origin as jsdom's), history entry replaced. */
function openAt(url: string): void {
  window.history.replaceState(null, '', url);
}

beforeEach(() => {
  window.localStorage.removeItem(AUTH_TOKEN_STORAGE_KEY);
});

afterEach(() => {
  openAt('/');
  vi.restoreAllMocks();
});

describe('establishAuth', () => {
  it('exchanges a fragment code, stores the token, and removes the code from the URL', async () => {
    openAt('/sessions/s1?tab=log#code=the-code');
    const exchange = vi.fn().mockResolvedValue('the-token');

    const state = await establishAuth(exchange);

    expect(state).toEqual({ kind: 'ready' });
    expect(exchange).toHaveBeenCalledWith('the-code');
    expect(storedAuthToken()).toBe('the-token');
    expect(window.location.hash).toBe('');
    expect(window.location.href).not.toContain('the-code');
    // The rest of the URL is kept.
    expect(window.location.pathname).toBe('/sessions/s1');
    expect(window.location.search).toBe('?tab=log');
  });

  it('uses a stored token when the URL carries no code', async () => {
    window.localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, 'kept');
    openAt('/');
    const exchange = vi.fn();

    const state = await establishAuth(exchange);

    expect(state).toEqual({ kind: 'ready' });
    expect(exchange).not.toHaveBeenCalled();
    expect(storedAuthToken()).toBe('kept');
  });

  it('asks for a link when there is neither a code nor a stored token', async () => {
    openAt('/');
    await expect(establishAuth(vi.fn())).resolves.toEqual({ kind: 'missing' });
  });

  it('reports a refused code, and still removes it from the URL', async () => {
    openAt('/#code=used');
    const exchange = vi
      .fn()
      .mockRejectedValue(new ApiError(401, 'Unauthorized'));

    const state = await establishAuth(exchange);

    expect(state).toEqual({ kind: 'code-refused' });
    expect(window.location.hash).toBe('');
    expect(storedAuthToken()).toBe('');
  });

  it('reports an exchange that failed for another reason', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    openAt('/#code=c');
    const exchange = vi.fn().mockRejectedValue(new TypeError('network down'));

    const state = await establishAuth(exchange);

    expect(state).toEqual({ kind: 'exchange-failed', message: 'network down' });
  });

  it('ignores a fragment without a code', async () => {
    window.localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, 'kept');
    openAt('/#end');
    const exchange = vi.fn();

    await expect(establishAuth(exchange)).resolves.toEqual({ kind: 'ready' });
    expect(exchange).not.toHaveBeenCalled();
    expect(window.location.hash).toBe('#end');
  });
});
