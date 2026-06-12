import { expect, type Page } from '@playwright/test';
import type {
  MessagesResponse,
  SendsResponse,
  SessionsResponse,
} from '@delta/wire-gen';

/**
 * REST observation helpers for the fake-mode suite.
 *
 * The suite's Vite dev server proxies `/api` to the run's real backend, so
 * `page.request` (same baseURL as the page) reads the server's state directly.
 * The reconnect specs use this to observe what the server knows *while the
 * page's live socket is down* — e.g. "the turn completed", "the send is still
 * open" — without depending on wall-clock sleeps, and then assert that the UI
 * converges to that observed truth after the socket returns.
 */

/** The ids a spec needs to address its own session over REST. */
export interface SessionHandle {
  id: string;
  mainThreadId: number;
}

/**
 * The most recently active session — the one the calling spec just started
 * (the suite runs specs serially, each against its own fresh session, and
 * `GET /api/sessions` orders by recent activity).
 */
export async function latestSession(page: Page): Promise<SessionHandle> {
  const response = await page.request.get('/api/sessions');
  expect(response.ok()).toBe(true);
  const body = (await response.json()) as SessionsResponse;
  expect(body.sessions.length).toBeGreaterThan(0);
  const item = body.sessions[0];
  return { id: item.session.id, mainThreadId: item.main_thread_id };
}

/** A session's open sends and turn state (`GET /api/sessions/{id}/sends`). */
export async function fetchSends(
  page: Page,
  sessionId: string,
): Promise<SendsResponse> {
  const response = await page.request.get(
    `/api/sessions/${encodeURIComponent(sessionId)}/sends`,
  );
  expect(response.ok()).toBe(true);
  return (await response.json()) as SendsResponse;
}

/** How many messages a thread holds (`GET /api/threads/{id}/messages`). */
export async function fetchMessageCount(
  page: Page,
  threadId: number,
): Promise<number> {
  const response = await page.request.get(`/api/threads/${threadId}/messages`);
  expect(response.ok()).toBe(true);
  const body = (await response.json()) as MessagesResponse;
  return body.messages.length;
}
