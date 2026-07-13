import { describe, expect, it } from 'vitest';
import type { HttpHandler } from 'msw';
import type {
  GitBranchesResponse,
  GitRepoResponse,
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
  const handler = handlers.find(
    (h) =>
      h.info.method === 'POST' && String(h.info.path).endsWith(pathSuffix),
  );
  if (!handler) {
    throw new Error(`POST handler ending in ${pathSuffix} not found`);
  }
  const request = new Request(url, {
    method: 'POST',
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
  it('mints real session/thread/send ids and keeps the spawning row unlisted', async () => {
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

    // The spawning row stays out of the session list…
    const page = await getSessionsPage(httpHandlers, '?limit=100');
    expect(page.sessions.some((s) => s.session.id === sessionId)).toBe(false);

    // …but its open sends are already queryable, so the pending chip can
    // render from "server" state across the spawn window.
    const sends = (await getOpenSends(httpHandlers, sessionId)).sends;
    expect(sends.map((s) => s.text)).toEqual(['kick off']);
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

    applyEvent({ kind: 'session_registered', session_id: registered });
    const page = await getSessionsPage(httpHandlers, '?limit=100');
    const listed = page.sessions.find((s) => s.session.id === registered);
    expect(listed?.open).toBe(true);

    // The reaped spawn disappears entirely: 404, exactly as the real server
    // answers after deleting the contentless failed session.
    applyEvent({
      kind: 'spawn_failed',
      session_id: failed,
      pane_token: 'pane-x',
    });
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
});
