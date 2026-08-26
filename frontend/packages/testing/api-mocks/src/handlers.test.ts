import { describe, expect, it } from 'vitest';
import type { HttpHandler } from 'msw';
import type {
  CloneRoot,
  GitBranchesResponse,
  GitRepoResponse,
  PromptTemplate,
  PromptTemplatesResponse,
  SendResponse,
  SendsResponse,
  SessionsResponse,
} from '@delta/wire-gen';
import { createHandlers, createMockApi } from './handlers';
import {
  mockSpawnSessionId,
  MOCK_GIT_REPO_ROOT,
  SESSIONS_PAGE_SIZE,
  SESSION_ID,
  SESSION_ID_2,
  SESSION_ID_3,
  MAIN_THREAD_ID,
  SESSION_3_MAIN_THREAD_ID,
} from './fixtures';

/**
 * The `GET /api/sessions` mock is cursor-paginated so the infinite-scroll path
 * (a non-null `next_cursor`, then a terminal `null`) is exercised without a
 * backend. These tests drive the handler directly and walk the cursor chain to
 * verify every seeded session is reachable exactly once, in order, across pages.
 */

/** Resolve the `GET /api/sessions` handler for a given query string. */
async function getSessionsPage(
  handlers: HttpHandler[],
  query = '',
): Promise<SessionsResponse> {
  const handler = handlers.find(
    (h) => h.info.method === 'GET' && String(h.info.path).endsWith('/api/sessions'),
  );
  if (!handler) {
    throw new Error('GET /api/sessions handler not found');
  }
  const request = new Request(`http://localhost/api/sessions${query}`);
  const result = await handler.run({ request, requestId: 'test' });
  const response = result?.response;
  if (!response) {
    throw new Error('handler did not produce a response');
  }
  return (await response.json()) as SessionsResponse;
}

describe('GET /api/sessions mock pagination', () => {
  it('returns the first page with a non-null next_cursor when more remain', async () => {
    const handlers = createHandlers() as HttpHandler[];

    const first = await getSessionsPage(handlers);

    expect(first.sessions).toHaveLength(SESSIONS_PAGE_SIZE);
    expect(first.next_cursor).not.toBeNull();
  });

  it('walks the cursor chain to a terminal null, covering every session once', async () => {
    const handlers = createHandlers() as HttpHandler[];

    const seen: string[] = [];
    let cursor: string | null = null;
    let pages = 0;
    do {
      const query: string =
        cursor === null ? '' : `?cursor=${encodeURIComponent(cursor)}`;
      const page: SessionsResponse = await getSessionsPage(handlers, query);
      expect(page.sessions.length).toBeGreaterThan(0);
      expect(page.sessions.length).toBeLessThanOrEqual(SESSIONS_PAGE_SIZE);
      for (const item of page.sessions) {
        seen.push(item.session.id);
      }
      cursor = page.next_cursor;
      pages += 1;
    } while (cursor !== null);

    // More than one page, terminating cleanly.
    expect(pages).toBeGreaterThan(1);
    // Every session appears exactly once across the walk (no dupes, no gaps).
    expect(new Set(seen).size).toBe(seen.length);
    // The two detailed sessions are the most recently active, so they lead the
    // list (sess-mock-2 has the newest message, then sess-mock-1) ahead of the
    // older filler sessions.
    expect(seen.slice(0, 2)).toEqual(['sess-mock-2', 'sess-mock-1']);
  });

  it('honors an explicit limit', async () => {
    const handlers = createHandlers() as HttpHandler[];

    const page = await getSessionsPage(handlers, '?limit=1');

    expect(page.sessions).toHaveLength(1);
    expect(page.next_cursor).not.toBeNull();
  });
});

/** Run a POST handler selected by a path suffix, returning the raw response. */
async function runPost(
  handlers: HttpHandler[],
  pathSuffix: string,
  url: string,
  body?: unknown,
): Promise<Response> {
  return runWithMethod(handlers, 'POST', pathSuffix, url, body);
}

describe('resume-unavailable session mock', () => {
  it('refuses to open the resume-unavailable session with 409 resume_unavailable', async () => {
    const handlers = createHandlers() as HttpHandler[];

    const response = await runPost(
      handlers,
      '/open',
      `http://localhost/api/sessions/${SESSION_ID_3}/open`,
    );

    expect(response.status).toBe(409);
    const body = (await response.json()) as { code?: string };
    expect(body.code).toBe('resume_unavailable');
  });

  it('refuses a send to the resume-unavailable session with 409 resume_unavailable', async () => {
    const handlers = createHandlers() as HttpHandler[];

    const response = await runPost(handlers, '/api/sends', 'http://localhost/api/sends', {
      thread_id: SESSION_3_MAIN_THREAD_ID,
      text: 'resume please',
    });

    expect(response.status).toBe(409);
    const body = (await response.json()) as { code?: string };
    expect(body.code).toBe('resume_unavailable');
  });

  it('still opens a normal closed session (the gate is specific to resume-unavailable)', async () => {
    const handlers = createHandlers() as HttpHandler[];

    const response = await runPost(
      handlers,
      '/open',
      `http://localhost/api/sessions/${SESSION_ID_2}/open`,
    );

    expect(response.status).toBe(204);
  });
});

/** Run a GET handler selected by a path pattern suffix. */
async function runGet(
  handlers: HttpHandler[],
  pathSuffix: string,
  url: string,
): Promise<Response> {
  const handler = handlers.find(
    (h) => h.info.method === 'GET' && String(h.info.path).endsWith(pathSuffix),
  );
  if (!handler) {
    throw new Error(`GET handler ending in ${pathSuffix} not found`);
  }
  const result = await handler.run({ request: new Request(url), requestId: 'test' });
  const response = result?.response;
  if (!response) {
    throw new Error('handler did not produce a response');
  }
  return response;
}

/** A session's open sends via the mock `GET /api/sessions/{id}/sends`. */
async function getOpenSends(
  handlers: HttpHandler[],
  sessionId: string,
): Promise<SendsResponse> {
  const response = await runGet(
    handlers,
    '/sends',
    `http://localhost/api/sessions/${sessionId}/sends`,
  );
  expect(response.status).toBe(200);
  return (await response.json()) as SendsResponse;
}

describe('GET /api/sessions/{id}/sends mock', () => {
  it('lists only the session’s non-terminal sends, oldest first', async () => {
    const { handlers, applyEvent } = createMockApi();
    const httpHandlers = handlers as HttpHandler[];

    // Two sends into the open session; both list, in submit order.
    const first = (await (
      await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
        thread_id: MAIN_THREAD_ID,
        text: 'first',
      })
    ).json()) as SendResponse;
    await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
      thread_id: MAIN_THREAD_ID,
      text: 'second',
    });

    let sends = (await getOpenSends(httpHandlers, SESSION_ID)).sends;
    expect(sends.map((s) => s.text)).toEqual(['first', 'second']);
    expect(sends.every((s) => s.status === 'dispatched')).toBe(true);

    // turn_started resolves the named send only.
    applyEvent({
      kind: 'turn_started',
      session_id: SESSION_ID,
      send_id: first.send.id,
      matched_uuid: 'uuid-m1',
    });
    sends = (await getOpenSends(httpHandlers, SESSION_ID)).sends;
    expect(sends.map((s) => s.text)).toEqual(['second']);

    // turn_completed drains the rest of the session's open sends.
    applyEvent({
      kind: 'turn_completed',
      session_id: SESSION_ID,
      stop_reason: null,
    });
    sends = (await getOpenSends(httpHandlers, SESSION_ID)).sends;
    expect(sends).toEqual([]);
  });

  it('reports the turn as in_flight from turn_started until turn_completed', async () => {
    // The envelope must mirror the real server's turn phases across a full
    // turn: `awaiting_echo` while the dispatch is outstanding, `in_flight`
    // for the whole running turn (whose send is already `matched`, so the
    // send list alone would wrongly read as idle), and `idle` only after the
    // turn ends. The `in_flight` report is what the app's authoritative
    // turn re-seed reconciles against — an `idle` here mid-turn would wipe
    // the running flag the `turn_started` event just set.
    const { handlers, applyEvent } = createMockApi();
    const httpHandlers = handlers as HttpHandler[];

    const posted = (await (
      await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
        thread_id: MAIN_THREAD_ID,
        text: 'drive a turn',
      })
    ).json()) as SendResponse;

    let envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.turn.state).toBe('awaiting_echo');

    applyEvent({
      kind: 'turn_started',
      session_id: SESSION_ID,
      send_id: posted.send.id,
      thread_id: MAIN_THREAD_ID,
      matched_uuid: 'uuid-m1',
    });
    envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.turn).toEqual({
      state: 'in_flight',
      send_id: posted.send.id,
      thread_id: MAIN_THREAD_ID,
    });

    applyEvent({
      kind: 'turn_completed',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      stop_reason: null,
    });
    envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.turn).toEqual({
      state: 'idle',
      send_id: null,
      thread_id: null,
    });
  });

  it('reports the pending-permission queue as head plus depth', async () => {
    // The mock mirrors the real server's queue: several approvals can be
    // outstanding at once (a parallel tool-call fan-out), the envelope reports
    // the OLDEST as the head with the total depth beside it, and a resolution
    // retires only the request it names — so the next one becomes the head.
    const { handlers, applyEvent } = createMockApi();
    const httpHandlers = handlers as HttpHandler[];

    for (const [requestId, command] of [
      [11, 'cat a'],
      [12, 'cat b'],
      [13, 'cat c'],
    ] as const) {
      applyEvent({
        kind: 'permission_requested',
        session_id: SESSION_ID,
        request_id: requestId,
        tool_name: command,
        tool_input: '{}',
      });
    }

    let envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.permission?.request_id).toBe(11);
    expect(envelope.permission_count).toBe(3);

    // A middle request resolves: the head is untouched, the depth shrinks.
    applyEvent({
      kind: 'permission_resolved',
      session_id: SESSION_ID,
      request_id: 12,
    });
    envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.permission?.request_id).toBe(11);
    expect(envelope.permission_count).toBe(2);

    // The head resolves: the next request takes over.
    applyEvent({
      kind: 'permission_resolved',
      session_id: SESSION_ID,
      request_id: 11,
    });
    envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.permission?.request_id).toBe(13);
    expect(envelope.permission_count).toBe(1);

    // The turn ending sweeps whatever is left, as the server's runtime does.
    applyEvent({
      kind: 'turn_completed',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      stop_reason: null,
    });
    envelope = await getOpenSends(httpHandlers, SESSION_ID);
    expect(envelope.permission).toBeNull();
    expect(envelope.permission_count).toBe(0);
  });

  it('responds 404 for an unknown session id', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runGet(
      handlers,
      '/sends',
      'http://localhost/api/sessions/ghost/sends',
    );
    expect(response.status).toBe(404);
  });
});

describe('new-session send mock (eager rows)', () => {
  it('mints real session/thread/send ids and lists the spawning row', async () => {
    const { handlers } = createMockApi();
    const httpHandlers = handlers as HttpHandler[];

    const body = (await (
      await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
        new_session: true,
        text: 'kick off',
      })
    ).json()) as SendResponse;

    const sessionId = mockSpawnSessionId(1);
    expect(body.send.session_id).toBe(sessionId);
    expect(body.send.thread_id).toBeGreaterThan(0);
    expect(body.send.id).toBeGreaterThan(0);
    expect(body.send.status).toBe('dispatched');

    // The session is listed from the moment the send was accepted, reading
    // `spawning` and not yet open — that is the row the workspace focuses.
    const page = await getSessionsPage(httpHandlers, '?limit=100');
    const listed = page.sessions.find((s) => s.session.id === sessionId);
    expect(listed?.session.status).toBe('spawning');
    expect(listed?.open).toBe(false);

    // Its open sends are queryable too, so the first prompt's chip renders
    // from "server" state across the spawn window.
    const sends = (await getOpenSends(httpHandlers, sessionId)).sends;
    expect(sends.map((s) => s.text)).toEqual(['kick off']);
  });

  it('refuses a second send while the row is still spawning', async () => {
    // Pins the mock to the real server's refusal: without it a frontend that
    // stopped disabling the starting composer would still pass every suite
    // (see the guard in `handlers.ts` for why the server refuses).
    const { handlers, applyEvent } = createMockApi();
    const httpHandlers = handlers as HttpHandler[];

    const first = (await (
      await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
        new_session: true,
        text: 'kick off',
      })
    ).json()) as SendResponse;

    const refused = await runPost(
      httpHandlers,
      '/api/sends',
      'http://localhost/api/sends',
      { thread_id: first.send.thread_id, text: 'too early' },
    );
    expect(refused.status).toBe(409);
    expect(((await refused.json()) as { code?: string }).code).toBe(
      'session_spawning',
    );

    // The gate lifts with the spawn window: once the launch registers, the
    // same send goes through.
    applyEvent({
      kind: 'session_registered',
      session_id: first.send.session_id,
    });
    const accepted = await runPost(
      httpHandlers,
      '/api/sends',
      'http://localhost/api/sends',
      { thread_id: first.send.thread_id, text: 'now you are up' },
    );
    expect(accepted.status).toBe(201);
  });

  it('activates the row on session_registered and deletes it on spawn_failed', async () => {
    const { handlers, applyEvent } = createMockApi();
    const httpHandlers = handlers as HttpHandler[];

    await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
      new_session: true,
      text: 'will register',
    });
    await runPost(httpHandlers, '/api/sends', 'http://localhost/api/sends', {
      new_session: true,
      text: 'will fail',
    });
    const registered = mockSpawnSessionId(1);
    const failed = mockSpawnSessionId(2);

    // Both rows are listed from acceptance; registration only flips one of
    // them to `active` + open.
    let page = await getSessionsPage(httpHandlers, '?limit=100');
    expect(
      page.sessions
        .filter((s) => [registered, failed].includes(s.session.id))
        .map((s) => s.session.status),
    ).toEqual(['spawning', 'spawning']);

    applyEvent({ kind: 'session_registered', session_id: registered });
    page = await getSessionsPage(httpHandlers, '?limit=100');
    const listed = page.sessions.find((s) => s.session.id === registered);
    expect(listed?.open).toBe(true);
    expect(listed?.session.status).toBe('active');

    // The reaped spawn disappears entirely: it leaves the session list, and
    // its sends answer 404 — exactly as the real server does after deleting
    // the contentless failed session.
    applyEvent({
      kind: 'spawn_failed',
      session_id: failed,
      pane_token: 'pane-x',
    });
    page = await getSessionsPage(httpHandlers, '?limit=100');
    expect(page.sessions.some((s) => s.session.id === failed)).toBe(false);
    const response = await runGet(
      httpHandlers,
      '/sends',
      `http://localhost/api/sessions/${failed}/sends`,
    );
    expect(response.status).toBe(404);
  });
});

describe('git workdir mocks', () => {
  it('reports the repo root and default branch for a path in the mock repo', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runGet(
      handlers,
      '/api/workdir/git',
      `http://localhost/api/workdir/git?path=${encodeURIComponent(MOCK_GIT_REPO_ROOT)}`,
    );
    expect(response.status).toBe(200);
    const body = (await response.json()) as GitRepoResponse;
    expect(body.repo_root).toBe(MOCK_GIT_REPO_ROOT);
    expect(body.default_branch).toBe('main');
  });

  it('reports a null repo root for a non-git path (never errors)', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runGet(
      handlers,
      '/api/workdir/git',
      'http://localhost/api/workdir/git?path=%2Fhome%2Fdev%2Fscratch',
    );
    expect(response.status).toBe(200);
    const body = (await response.json()) as GitRepoResponse;
    expect(body.repo_root).toBeNull();
    expect(body.default_branch).toBeNull();
  });

  it('lists remote branches for a repo path', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runGet(
      handlers,
      '/api/workdir/git/branches',
      `http://localhost/api/workdir/git/branches?path=${encodeURIComponent(MOCK_GIT_REPO_ROOT)}`,
    );
    expect(response.status).toBe(200);
    const body = (await response.json()) as GitBranchesResponse;
    expect(body.default_branch).toBe('main');
    expect(body.remote_branches).toEqual(['main', 'develop', 'release/1.0']);
  });

  it('rejects the branches endpoint for a non-git path with 400', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runGet(
      handlers,
      '/api/workdir/git/branches',
      'http://localhost/api/workdir/git/branches?path=%2Fhome%2Fdev%2Fscratch',
    );
    expect(response.status).toBe(400);
  });

  // `path` is required on both git endpoints, and the server decides that on a
  // trimmed value (`WorkdirGitQuery::require_path`). These pin the mock to the
  // same rule: a mock that answers 200 where the real server answers 400 lets a
  // frontend bug pass every mock-backed test and fail only against the backend.
  it.each([
    ['omitted', ''],
    ['empty', '?path='],
    ['whitespace-only', '?path=%20%20'],
  ])('rejects the repo probe for a %s path with 400', async (_label, query) => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runGet(
      handlers,
      '/api/workdir/git',
      `http://localhost/api/workdir/git${query}`,
    );
    expect(response.status).toBe(400);
  });

  it.each([
    ['omitted', ''],
    ['empty', '?path='],
    ['whitespace-only', '?path=%20%20'],
  ])(
    'rejects the branches endpoint for a %s path with 400',
    async (_label, query) => {
      const handlers = createHandlers() as HttpHandler[];
      const response = await runGet(
        handlers,
        '/api/workdir/git/branches',
        `http://localhost/api/workdir/git/branches${query}`,
      );
      expect(response.status).toBe(400);
    },
  );

  it('trims a padded path before resolving it, as the server does', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const padded = encodeURIComponent(`  ${MOCK_GIT_REPO_ROOT}  `);
    const response = await runGet(
      handlers,
      '/api/workdir/git',
      `http://localhost/api/workdir/git?path=${padded}`,
    );
    expect(response.status).toBe(200);
    const body = (await response.json()) as GitRepoResponse;
    expect(body.repo_root).toBe(MOCK_GIT_REPO_ROOT);
  });
});

/**
 * The clone-root mock canonicalises a submitted `path` exactly as the server's
 * `create_clone_root` does — trailing slashes stripped, the bare root left
 * alone, the result rejected when it comes out blank. These assert the same
 * input classes the server-side tests in `delta-server`'s `app.rs` name.
 */
describe('clone-root mock canonicalisation', () => {
  it.each([
    ['empty', ''],
    ['whitespace-only', '   '],
    ['double-slash', '//'],
    ['all-slashes', '///'],
  ])('rejects a %s path with 400', async (_label, path) => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runPost(
      handlers,
      '/api/clone-roots',
      'http://localhost/api/clone-roots',
      { path },
    );
    expect(response.status).toBe(400);
  });

  it('registers the bare filesystem root, which is not blank', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runPost(
      handlers,
      '/api/clone-roots',
      'http://localhost/api/clone-roots',
      { path: '/' },
    );
    expect(response.status).toBe(201);
    const body = (await response.json()) as CloneRoot;
    expect(body.path).toBe('/');
  });

  it('strips a trailing slash so both spellings are the same row', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const created = await runPost(
      handlers,
      '/api/clone-roots',
      'http://localhost/api/clone-roots',
      { path: '/home/dev/projects/' },
    );
    expect(created.status).toBe(201);
    const body = (await created.json()) as CloneRoot;
    expect(body.path).toBe('/home/dev/projects');

    // The unslashed spelling now collides with it, which is what "the same row"
    // means on a server whose clone-root table keys on the path.
    const duplicate = await runPost(
      handlers,
      '/api/clone-roots',
      'http://localhost/api/clone-roots',
      { path: '/home/dev/projects' },
    );
    expect(duplicate.status).toBe(409);
    const duplicateBody = (await duplicate.json()) as { code?: string };
    expect(duplicateBody.code).toBe('clone_root_duplicate');
  });

  it('rejects a relative path with 400', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runPost(
      handlers,
      '/api/clone-roots',
      'http://localhost/api/clone-roots',
      { path: 'home/dev/projects' },
    );
    expect(response.status).toBe(400);
  });
});

/**
 * Run a handler selected by method and path suffix, returning the raw response.
 * Backs {@link runPost}; {@link runGet} stays separate because a GET carries no
 * body and no content-type.
 */
async function runWithMethod(
  handlers: HttpHandler[],
  method: 'POST' | 'PATCH' | 'DELETE',
  pathSuffix: string,
  url: string,
  body?: unknown,
): Promise<Response> {
  const handler = handlers.find(
    (h) => h.info.method === method && String(h.info.path).endsWith(pathSuffix),
  );
  if (!handler) {
    throw new Error(`${method} handler ending in ${pathSuffix} not found`);
  }
  const request = new Request(url, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const result = await handler.run({ request, requestId: 'test' });
  const response = result?.response;
  if (!response) {
    throw new Error('handler did not produce a response');
  }
  return response;
}

/** The registered prompt templates via the mock `GET /api/prompt-templates`. */
async function getPromptTemplates(
  handlers: HttpHandler[],
): Promise<PromptTemplate[]> {
  const response = await runGet(
    handlers,
    '/api/prompt-templates',
    'http://localhost/api/prompt-templates',
  );
  expect(response.status).toBe(200);
  const body = (await response.json()) as PromptTemplatesResponse;
  return body.prompt_templates;
}

/**
 * The prompt-template mock reproduces the real server's CRUD contract, so the
 * Settings editor and the composer's insert menu can be driven end to end with
 * no backend: oldest-first ordering, a verbatim body, the blank-field 400, an
 * in-place edit that re-stamps `updated_at`, and an idempotent delete.
 */
describe('prompt-template mock CRUD', () => {
  it('seeds two templates, one of them long and multi-line', async () => {
    const templates = await getPromptTemplates(
      createHandlers() as HttpHandler[],
    );

    expect(templates).toHaveLength(2);
    // Oldest first, as the server returns them.
    expect(templates.map((t) => t.id)).toEqual([1, 2]);
    // One seed is realistic list-rendering material rather than a one-liner:
    // several paragraphs and well past 200 characters.
    const long = templates[1];
    expect(long.text.length).toBeGreaterThan(200);
    expect(long.text.split('\n').length).toBeGreaterThan(3);
  });

  it('creates, lists, edits in place, and deletes a template', async () => {
    const handlers = createHandlers() as HttpHandler[];

    // The body deliberately ends with a newline: the mock stores it verbatim,
    // as the server does.
    const created = await runPost(
      handlers,
      '/api/prompt-templates',
      'http://localhost/api/prompt-templates',
      { label: 'Ship it', text: 'Merge, then update the plan doc.\n' },
    );
    expect(created.status).toBe(201);
    const template = (await created.json()) as PromptTemplate;
    expect(template.text).toBe('Merge, then update the plan doc.\n');
    expect(template.updated_at).toBe(template.created_at);

    expect(await getPromptTemplates(handlers)).toHaveLength(3);

    const updated = await runWithMethod(
      handlers,
      'PATCH',
      '/api/prompt-templates/:id',
      `http://localhost/api/prompt-templates/${template.id}`,
      { label: 'Ship it carefully', text: 'Merge once green.' },
    );
    expect(updated.status).toBe(200);
    const edited = (await updated.json()) as PromptTemplate;
    expect(edited.id).toBe(template.id);
    expect(edited.label).toBe('Ship it carefully');
    expect(edited.created_at).toBe(template.created_at);

    // The edit replaced the row rather than adding one.
    const listed = await getPromptTemplates(handlers);
    expect(listed).toHaveLength(3);
    expect(listed.find((t) => t.id === template.id)?.label).toBe(
      'Ship it carefully',
    );

    const deleted = await runWithMethod(
      handlers,
      'DELETE',
      '/api/prompt-templates/:id',
      `http://localhost/api/prompt-templates/${template.id}`,
    );
    expect(deleted.status).toBe(204);
    expect(await getPromptTemplates(handlers)).toHaveLength(2);

    // Deleting it again is an idempotent no-op, like the real server.
    const again = await runWithMethod(
      handlers,
      'DELETE',
      '/api/prompt-templates/:id',
      `http://localhost/api/prompt-templates/${template.id}`,
    );
    expect(again.status).toBe(204);
  });

  it.each([
    ['blank label', { label: '   ', text: 'body' }],
    ['blank text', { label: 'Label', text: '\n\t ' }],
  ])('rejects a create with a %s with 400', async (_label, body) => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runPost(
      handlers,
      '/api/prompt-templates',
      'http://localhost/api/prompt-templates',
      body,
    );
    expect(response.status).toBe(400);
    expect(await getPromptTemplates(handlers)).toHaveLength(2);
  });

  it('answers 404 when editing an unknown id', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runWithMethod(
      handlers,
      'PATCH',
      '/api/prompt-templates/:id',
      'http://localhost/api/prompt-templates/9999',
      { label: 'Label', text: 'text' },
    );
    expect(response.status).toBe(404);
  });

  it('answers 400 rather than 404 when a blank edit names an unknown id', async () => {
    // The server validates in the use case, before the store is consulted, so
    // the blank field wins over the missing row. The mock must agree, or a test
    // written against it would expect the wrong status from the real server.
    const handlers = createHandlers() as HttpHandler[];
    const response = await runWithMethod(
      handlers,
      'PATCH',
      '/api/prompt-templates/:id',
      'http://localhost/api/prompt-templates/9999',
      { label: '   ', text: 'text' },
    );
    expect(response.status).toBe(400);
  });
});
