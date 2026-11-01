import { expect, type Page } from '@playwright/test';
import type {
  MessagesResponse,
  SendsResponse,
  SessionsResponse,
} from '@delta/wire-gen';
import { focusedSessionNode } from './app';

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
 * The session the page is focused on — the one the calling spec just started,
 * since `startNewSession` returns only once the workspace has focused it.
 *
 * The id is read from the focused navigator card, never inferred from the
 * server's ordering. The suite shares one server, and a session an earlier
 * spec left running keeps producing activity after that spec ends (e.g. the
 * `first-send` scenario's reply, scripted 1.5 s after its prompt, lands while
 * the next spec is already running). `GET /api/sessions` orders by most recent
 * activity, so its first row can be another spec's session, whose REST state
 * then never changes the way the calling spec waits for.
 */
export async function focusedSession(page: Page): Promise<SessionHandle> {
  const card = page
    .getByTestId('session-card')
    .filter({ has: focusedSessionNode(page) });
  await expect(card).toHaveAttribute('data-session-id', /.+/);
  const id = await card.getAttribute('data-session-id');
  // The first page of the list carries every live session, and the session
  // a spec just started is live.
  const response = await page.request.get('/api/sessions');
  expect(response.ok()).toBe(true);
  const body = (await response.json()) as SessionsResponse;
  const item = body.sessions.find((entry) => entry.session.id === id);
  if (item === undefined) {
    throw new Error(`the focused session ${id} is not in the session list`);
  }
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

/** Whether `GET /api/sessions` lists the session as open. */
export async function sessionIsOpen(
  page: Page,
  sessionId: string,
): Promise<boolean> {
  const response = await page.request.get('/api/sessions');
  expect(response.ok()).toBe(true);
  const body = (await response.json()) as SessionsResponse;
  const item = body.sessions.find((entry) => entry.session.id === sessionId);
  expect(item, `session ${sessionId} is listed`).toBeDefined();
  return item?.open ?? false;
}
