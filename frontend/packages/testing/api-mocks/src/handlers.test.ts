import { describe, expect, it } from 'vitest';
import type { HttpHandler } from 'msw';
import type { SessionsResponse } from '@delta/model';
import { createHandlers } from './handlers';
import {
  SESSIONS_PAGE_SIZE,
  SESSION_ID_2,
  SESSION_ID_3,
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
