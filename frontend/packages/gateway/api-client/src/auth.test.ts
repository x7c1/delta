import { describe, expect, it, vi } from 'vitest';
import { ApiClient } from './http';
import { ApiError } from './errors';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('ApiClient one-time code exchange', () => {
  it('posts the code without a token and returns the token', async () => {
    const fetchFn = vi.fn().mockResolvedValue(jsonResponse({ token: 'tok' }));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(client.exchangeAuthCode('the-code')).resolves.toBe('tok');

    const [url, init] = fetchFn.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('http://localhost/api/auth/exchange');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ code: 'the-code' });
    expect(new Headers(init.headers).get('Authorization')).toBeNull();
  });

  it('throws a 401 ApiError for a refused code without reporting it', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(new Response(null, { status: 401 }));
    const onUnauthorized = vi.fn();
    const client = new ApiClient({ fetchFn, onUnauthorized });

    const error = await client.exchangeAuthCode('used').catch((e) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(401);
    expect(onUnauthorized).not.toHaveBeenCalled();
  });
});

describe('ApiClient onUnauthorized', () => {
  it('is told about a 401 answer, and the request still fails', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(new Response(null, { status: 401 }));
    const onUnauthorized = vi.fn();
    const client = new ApiClient({ fetchFn, token: 'stale', onUnauthorized });

    await expect(client.getVersion()).rejects.toBeInstanceOf(ApiError);
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });

  it('is not told about other failures', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ error: 'nope' }, 500));
    const onUnauthorized = vi.fn();
    const client = new ApiClient({ fetchFn, token: 't', onUnauthorized });

    await expect(client.getVersion()).rejects.toBeInstanceOf(ApiError);
    expect(onUnauthorized).not.toHaveBeenCalled();
  });
});
