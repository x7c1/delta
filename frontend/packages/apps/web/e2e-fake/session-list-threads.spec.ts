import { test, expect } from './support/fixtures';
import { startNewSession, waitForSettledApp } from './support/app';
import type {
  MessagesResponse,
  SessionsResponse,
} from '@delta/wire-gen';

/**
 * The session list carries every session's threads, so a launch draws each
 * navigator row's sub-thread tree without a request per row.
 *
 * Scenario `session-list-threads`: the fake answers the first prompt and then
 * holds the turn open. The spec starts several sessions that way and branches
 * each one over REST — mid-turn, so the branch send is held `queued` and only
 * its thread is created. It then reloads the app and counts the
 * `GET /api/sessions/{id}/threads` requests the launch makes: at most the
 * focused session's one, while every row still shows its own branch.
 */

const SESSION_COUNT = 3;

/** `GET /api/sessions/{id}/threads`, matched on the request path. */
const THREADS_PATH = /^\/api\/sessions\/[^/]+\/threads$/;

test('a launch draws every row’s thread tree from the session list', async ({
  page,
}) => {
  await page.goto('/');

  const listSessions = async (): Promise<SessionsResponse> => {
    const response = await page.request.get('/api/sessions');
    expect(response.ok()).toBe(true);
    return (await response.json()) as SessionsResponse;
  };

  const sessions: { id: string; branchTitle: string }[] = [];
  for (let n = 0; n < SESSION_COUNT; n += 1) {
    const before = new Set(
      (await listSessions()).sessions.map((entry) => entry.session.id),
    );
    await startNewSession(page, `session-list-threads ${n}`);
    // The reply lands and the turn stays in flight.
    await expect(page.getByTestId('message-item')).toHaveCount(2);

    // The session just started is the one listed now that was not before.
    // (`created_at` has one-second resolution, so it cannot tell sessions
    // started in the same second apart.)
    const started = (await listSessions()).sessions.filter(
      (entry) => !before.has(entry.session.id),
    );
    expect(started, 'exactly one session was started').toHaveLength(1);
    const item = started[0];

    // Branch it off the reply. The turn is still open, so the send is held
    // and nothing reaches the fake; the branch thread exists from the POST on.
    const messagesResponse = await page.request.get(
      `/api/threads/${item.main_thread_id}/messages`,
    );
    expect(messagesResponse.ok()).toBe(true);
    const { messages } = (await messagesResponse.json()) as MessagesResponse;
    const reply = messages.find((message) => message.role === 'assistant');
    expect(reply, 'the reply was ingested').toBeDefined();
    const branchTitle = `listed branch ${n}`;
    const sendResponse = await page.request.post('/api/sends', {
      data: {
        thread_id: item.main_thread_id,
        text: `follow up ${n}`,
        locator_quote: branchTitle,
        semantic_parent_uuid: reply?.uuid,
      },
    });
    expect(sendResponse.status()).toBe(201);
    sessions.push({ id: item.session.id, branchTitle });
  }

  // Count the thread-tree requests the next launch makes.
  const threadRequests: string[] = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'GET' && THREADS_PATH.test(path)) {
      threadRequests.push(path);
    }
  });
  await page.reload();
  await waitForSettledApp(page);

  // Every row — focused or not — shows its own branch in its thread tree.
  for (const { branchTitle } of sessions) {
    await expect(
      page.getByRole('listitem').filter({ hasText: branchTitle }).first(),
    ).toBeVisible();
  }
  // Let the launch's remaining requests (if any) go out before counting.
  await page.waitForLoadState('networkidle');
  expect(
    threadRequests.length,
    `thread-tree requests at launch: ${threadRequests.join(', ')}`,
  ).toBeLessThanOrEqual(1);

  // Close the sessions so their held turns do not lead later specs' lists.
  for (const { id } of sessions) {
    const response = await page.request.post(
      `/api/sessions/${encodeURIComponent(id)}/close`,
      { data: {} },
    );
    expect(response.ok()).toBe(true);
  }
});
