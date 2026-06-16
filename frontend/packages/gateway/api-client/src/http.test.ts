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

  it('fetches a session’s open sends by id', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        sends: [
          {
            id: 7,
            session_id: 'sess-1',
            thread_id: 3,
            semantic_parent_uuid: null,
            text: 'hi',
            locator_quote: null,
            status: 'queued',
            matched_uuid: null,
            created_at: '2026-01-01T00:00:00Z',
          },
        ],
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getSessionSends('sess-1');

    expect(result.sends).toHaveLength(1);
    expect(result.sends[0].status).toBe('queued');
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/sessions/sess-1/sends',
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
            status: 'dispatched',
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
            status: 'dispatched',
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
            status: 'dispatched',
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

  it('serializes a new-session send carrying a working directory', async () => {
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
            status: 'dispatched',
            matched_uuid: null,
            created_at: '2026-01-01T00:00:00Z',
          },
        },
        201,
      ),
    );
    const client = new ApiClient({ fetchFn });

    await client.createSend({
      new_session: true,
      text: 'first',
      workdir: '/projects/app',
    });

    // The chosen workdir must survive serialization onto the new-session body.
    const [, init] = fetchFn.mock.calls[0];
    expect(JSON.parse(init.body)).toEqual({
      new_session: true,
      text: 'first',
      workdir: '/projects/app',
    });
  });

  it('lists a working directory at the default ($HOME) path', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        path: '/home/dev',
        parent: '/home',
        entries: [
          { name: 'projects', path: '/home/dev/projects' },
          { name: 'scratch', path: '/home/dev/scratch' },
        ],
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getWorkdirList();

    expect(result.path).toBe('/home/dev');
    expect(result.parent).toBe('/home');
    expect(result.entries).toHaveLength(2);
    expect(result.entries[0]).toEqual({
      name: 'projects',
      path: '/home/dev/projects',
    });
    // No path param: the default directory is requested with a bare path.
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/workdir/list',
      undefined,
    );
  });

  it('encodes the path query when listing a specific directory', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        path: '/home/dev/my projects',
        parent: '/home/dev',
        entries: [],
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await client.getWorkdirList('/home/dev/my projects');

    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/workdir/list?path=%2Fhome%2Fdev%2Fmy%20projects',
      undefined,
    );
  });

  it('fetches the recently-used working directories', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        workdirs: [
          { path: '/home/dev/projects/delta', last_used_at: '2026-01-03T00:00:00Z' },
          { path: '/home/dev/scratch', last_used_at: null },
        ],
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getWorkdirRecent();

    expect(result.workdirs).toHaveLength(2);
    expect(result.workdirs[0].path).toBe('/home/dev/projects/delta');
    expect(result.workdirs[1].last_used_at).toBeNull();
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/workdir/recent',
      undefined,
    );
  });

  it('fetches whether a directory is a git repository, encoding the path', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        repo_root: '/home/dev/projects/delta',
        default_branch: 'main',
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getGitRepoInfo('/home/dev/my repo');

    expect(result.repo_root).toBe('/home/dev/projects/delta');
    expect(result.default_branch).toBe('main');
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/workdir/git?path=%2Fhome%2Fdev%2Fmy%20repo',
      undefined,
    );
  });

  it('fetches the remote branches of a repository, encoding the path', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        default_branch: 'main',
        remote_branches: ['main', 'develop', 'release/1.0'],
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getGitBranches('/home/dev/my repo');

    expect(result.default_branch).toBe('main');
    expect(result.remote_branches).toEqual(['main', 'develop', 'release/1.0']);
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/workdir/git/branches?path=%2Fhome%2Fdev%2Fmy%20repo',
      undefined,
    );
  });

  it('surfaces a 400 from the branches endpoint as an ApiError', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ error: 'not a git repository' }, 400));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(client.getGitBranches('/home/dev/plain')).rejects.toThrow(
      ApiError,
    );
  });

  it('answers a pending question with the per-question selection indices', async () => {
    const fetchFn = vi.fn().mockResolvedValue(noContent());
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(
      client.answerQuestion('sess-1', 5, [[0], [2, 1]]),
    ).resolves.toBeUndefined();

    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/sessions/sess-1/questions/5/answer');
    expect(init.method).toBe('POST');
    expect(init.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(JSON.parse(init.body)).toEqual({ selections: [[0], [2, 1]] });
  });

  it('cancels a pending question, carrying the request id in the body', async () => {
    const fetchFn = vi.fn().mockResolvedValue(noContent());
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(
      client.cancelQuestion('sess-1', 5),
    ).resolves.toBeUndefined();

    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/sessions/sess-1/questions/cancel');
    expect(init.method).toBe('POST');
    expect(init.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(JSON.parse(init.body)).toEqual({ request_id: 5 });
  });

  it('cancels a queued send, posting to the send-scoped cancel route', async () => {
    const fetchFn = vi.fn().mockResolvedValue(noContent());
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(client.cancelSend(7)).resolves.toBeUndefined();

    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/sends/7/cancel');
    expect(init.method).toBe('POST');
  });

  it('surfaces send_not_cancellable as an ApiError code', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse(
        { error: 'send 7 is not cancellable', code: 'send_not_cancellable' },
        409,
      ),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(client.cancelSend(7)).rejects.toMatchObject({
      status: 409,
      code: 'send_not_cancellable',
    } satisfies Partial<ApiError>);
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

  it('surfaces the machine-readable error code from a send response', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse(
        { error: 'session cannot be resumed', code: 'resume_unavailable' },
        409,
      ),
    );
    const client = new ApiClient({ fetchFn });

    await expect(
      client.createSend({ thread_id: 1, text: 'hi' }),
    ).rejects.toMatchObject({
      status: 409,
      message: 'session cannot be resumed',
      code: 'resume_unavailable',
    } satisfies Partial<ApiError>);
  });

  it('surfaces the error code from an open (204-style) endpoint too', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse(
        { error: 'session cannot be resumed', code: 'resume_unavailable' },
        409,
      ),
    );
    const client = new ApiClient({ fetchFn });

    await expect(client.openSession('gone')).rejects.toMatchObject({
      status: 409,
      code: 'resume_unavailable',
    });
  });

  it('leaves the code undefined when the error body carries none', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValue(jsonResponse({ error: 'boom' }, 500));
    const client = new ApiClient({ fetchFn });

    const err = await client.getThreadMessages(1).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).code).toBeUndefined();
  });

  it('lists launch options from GET /api/launch-options', async () => {
    const fetchFn = vi.fn().mockResolvedValue(
      jsonResponse({
        launch_options: [
          {
            id: 1,
            label: 'plugins',
            name: '--plugin-dir',
            value: '/opt/p',
            created_at: '2026-01-01T00:00:00Z',
          },
        ],
      }),
    );
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.getLaunchOptions();
    expect(result.launch_options).toHaveLength(1);
    expect(result.launch_options[0].name).toBe('--plugin-dir');
    expect(fetchFn).toHaveBeenCalledWith(
      'http://localhost/api/launch-options',
      undefined,
    );
  });

  it('posts a new launch option and returns the created record', async () => {
    const created = {
      id: 7,
      label: null,
      name: '--permission-mode',
      value: 'auto',
      created_at: '2026-01-02T00:00:00Z',
    };
    const fetchFn = vi.fn().mockResolvedValue(jsonResponse(created, 201));
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    const result = await client.createLaunchOption({
      name: '--permission-mode',
      value: 'auto',
    });
    expect(result).toEqual(created);
    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/launch-options');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body)).toEqual({
      name: '--permission-mode',
      value: 'auto',
    });
  });

  it('deletes a launch option via DELETE /api/launch-options/{id}', async () => {
    const fetchFn = vi.fn().mockResolvedValue(noContent());
    const client = new ApiClient({ baseUrl: 'http://localhost', fetchFn });

    await expect(client.deleteLaunchOption(7)).resolves.toBeUndefined();
    const [url, init] = fetchFn.mock.calls[0];
    expect(url).toBe('http://localhost/api/launch-options/7');
    expect(init.method).toBe('DELETE');
  });
});
