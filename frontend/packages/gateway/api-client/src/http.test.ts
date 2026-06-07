import { describe, expect, it, vi } from 'vitest';
import { ApiClient, ApiError } from './http';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('ApiClient', () => {
  it('parses GET /api/session into a typed response', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        session: {
          id: 'sess-1',
          cwd: '/work/delta',
          transcript_path: '/t.jsonl',
          title: null,
          status: 'active',
          created_at: '2026-01-01T00:00:00Z',
        },
        main_thread_id: 1,
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getSession();

    expect(result.main_thread_id).toBe(1);
    expect(result.session.status).toBe('active');
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/session',
      undefined,
    );
  });

  it('posts to ensure the session and returns its lifecycle status', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ status: 'ready' }));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.ensureSession();

    expect(result.status).toBe('ready');
    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/session');
    expect(init.method).toBe('POST');
  });

  it('posts a send with a JSON content type and returns the pending send', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse(
        {
          send: {
            id: 7,
            session_id: 'sess-1',
            thread_id: 1,
            semantic_parent_uuid: null,
            text: 'hello',
            locator_quote: null,
            status: 'pending',
            matched_uuid: null,
            created_at: '2026-01-01T00:00:00Z',
          },
        },
        201,
      ),
    );
    const client = new ApiClient({ fetchFn });

    const result = await client.createSend({ thread_id: 1, text: 'hello' });

    expect(result.send.id).toBe(7);
    const [, init] = fetchFn.mock.calls[0];
    expect(init.method).toBe('POST');
    expect(init.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(JSON.parse(init.body)).toEqual({ thread_id: 1, text: 'hello' });
  });

  it('raises ApiError carrying the status and server error message', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ error: 'unknown thread' }, 404));
    const client = new ApiClient({ fetchFn });

    await expect(client.getThreadMessages(99)).rejects.toMatchObject({
      status: 404,
      message: 'unknown thread',
    } satisfies Partial<ApiError>);
  });
});
