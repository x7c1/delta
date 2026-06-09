import { describe, expect, it, vi } from 'vitest';
import { ApiClient, ApiError } from './http';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function noContent(): Response {
  return new Response(null, { status: 204 });
}

describe('ApiClient', () => {
  it('parses a GET /api/sessions page into a typed list with its cursor', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        sessions: [
          {
            session: {
              id: 'sess-1',
              cwd: '/work/delta',
              transcript_path: '/t.jsonl',
              title: null,
              status: 'active',
              created_at: '2026-01-01T00:00:00Z',
            },
            open: true,
            main_thread_id: 1,
            last_activity_at: '2026-01-01T00:01:01Z',
          },
        ],
        next_cursor: 'page-2',
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getSessions();

    expect(result.sessions).toHaveLength(1);
    expect(result.sessions[0].open).toBe(true);
    expect(result.sessions[0].main_thread_id).toBe(1);
    expect(result.sessions[0].last_activity_at).toBe('2026-01-01T00:01:01Z');
    expect(result.next_cursor).toBe('page-2');
    // No params: the first page is requested with a bare path.
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/sessions',
      undefined,
    );
  });

  it('builds the query string from cursor and limit, encoding each value', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ sessions: [], next_cursor: null }));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await client.getSessions({ cursor: 'a b/c', limit: 30 });

    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/sessions?cursor=a%20b%2Fc&limit=30',
      undefined,
    );
  });

  it('omits absent pagination params from the query string', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ sessions: [], next_cursor: null }));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await client.getSessions({ limit: 10 });

    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/sessions?limit=10',
      undefined,
    );
  });

  it('posts to spawn a new session and returns its lifecycle status', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ status: 'starting' }));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.newSession();

    expect(result.status).toBe('starting');
    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/sessions');
    expect(init.method).toBe('POST');
  });

  it('opens and closes a session via the 204 endpoints', async () => {
    const fetchFn = vi.fn().mockResolvedValue(noContent());
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(client.openSession('sess-1')).resolves.toBeUndefined();
    await expect(client.closeSession('sess-1')).resolves.toBeUndefined();

    expect(fetchFn).toHaveBeenNthCalledWith(
      1,
      'http://localhost/api/sessions/sess-1/open',
      { method: 'POST' },
    );
    expect(fetchFn).toHaveBeenNthCalledWith(
      2,
      'http://localhost/api/sessions/sess-1/close',
      { method: 'POST' },
    );
  });

  it('fetches a session thread tree by id', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ threads: [] }));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await client.getSessionThreads('sess-1');

    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/sessions/sess-1/threads',
      undefined,
    );
  });

  it('posts a thread-targeted send and returns the pending send', async () => {
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

  it('serializes a branch send with its semantic parent and locator quote', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse(
        {
          send: {
            id: 8,
            session_id: 'sess-1',
            thread_id: 1,
            semantic_parent_uuid: 'uuid-42',
            text: 'branch from here',
            locator_quote: 'the quoted line',
            status: 'pending',
            matched_uuid: null,
            created_at: '2026-01-01T00:00:00Z',
          },
        },
        201,
      ),
    );
    const client = new ApiClient({ fetchFn });

    await client.createSend({
      thread_id: 1,
      text: 'branch from here',
      semantic_parent_uuid: 'uuid-42',
      locator_quote: 'the quoted line',
    });

    // The branch fields must survive serialization; dropping either silently
    // turns a branch send into a plain trunk send.
    const [, init] = fetchFn.mock.calls[0];
    expect(JSON.parse(init.body)).toEqual({
      thread_id: 1,
      text: 'branch from here',
      semantic_parent_uuid: 'uuid-42',
      locator_quote: 'the quoted line',
    });
  });

  it('posts a new-session send with the new_session target', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse(
        {
          send: {
            id: 0,
            session_id: '',
            thread_id: 0,
            semantic_parent_uuid: null,
            text: 'first',
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

    const result = await client.createSend({ new_session: true, text: 'first' });

    // Synthetic id:0 send until the spawn binds.
    expect(result.send.id).toBe(0);
    const [, init] = fetchFn.mock.calls[0];
    expect(JSON.parse(init.body)).toEqual({ new_session: true, text: 'first' });
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

  it('raises ApiError for a 404 on a 204-style endpoint', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ error: 'unknown session' }, 404));
    const client = new ApiClient({ fetchFn });

    await expect(client.openSession('nope')).rejects.toMatchObject({
      status: 404,
    });
  });
});
