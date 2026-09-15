import type { SessionsResponse } from '@delta/wire-gen';
import { test, expect, type Page } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * A launch that never binds becomes an ordinary session the user can open,
 * read, retry and remove.
 *
 * Scenario `never-ready`: the fake skips its `SessionStart` hook and hangs, so
 * the spawn never binds and the backend's launch watchdog (its deadline shrunk
 * via DELTA_LAUNCH_DEADLINE_MS by the suite's server script) reaps it, kills its
 * pane and emits `spawn_failed`.
 *
 * What the server does with the row is the whole point: it KEEPS it, marked
 * `failed`, with the prompt that was never delivered still open against it. So
 * the failure has a place of its own — a screen that says why it did not start
 * and offers Retry and Remove — instead of a row that vanishes and forces every
 * surface around it to compensate for the absence.
 */

type ListedSession = SessionsResponse['sessions'][number];

/** Every session the server knows, newest-active first. */
async function listSessions(page: Page): Promise<ListedSession[]> {
  const response = await page.request.get('/api/sessions');
  expect(response.ok()).toBe(true);
  return ((await response.json()) as SessionsResponse).sessions;
}

/**
 * The one session in `status`, waited for — each spec's handle on its own row.
 * Read over REST rather than off the screen because the ids and the working
 * directory are what these specs compare, and neither is rendered.
 */
async function sessionWithStatus(
  page: Page,
  status: string,
): Promise<ListedSession['session']> {
  let match: ListedSession | undefined;
  await expect
    .poll(
      async () => {
        const sessions = await listSessions(page);
        match = sessions.find((item) => item.session.status === status);
        return match !== undefined;
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  return match!.session;
}

/** Whether the server still lists `sessionId` at all. */
async function stillListed(page: Page, sessionId: string): Promise<boolean> {
  return (await listSessions(page)).some(
    (item) => item.session.id === sessionId,
  );
}

/** The navigator card of the session showing `status`. */
function cardWithStatus(page: Page, status: string) {
  return page
    .locator('li')
    .filter({ has: page.getByRole('status', { name: status, exact: true }) });
}

test('a launch that fails while you are elsewhere turns its row failed, and opening it explains why', async ({
  page,
}) => {
  await page.goto('/');

  // The doomed launch first, so that the session started after it is the one
  // holding focus when the deadline passes.
  await startNewSession(page, 'never-ready hang at launch');
  const doomed = await sessionWithStatus(page, 'spawning');

  // A second session, which the workspace focuses as any new session — this is
  // what the user is reading while the first one gives up. The case used to
  // produce nothing at all here: the row simply disappeared and no notice was
  // raised anywhere.
  await startNewSession(page, 'first-send hello there');
  await expect(page.getByText('first-send hello there').first()).toBeVisible({
    timeout: 15_000,
  });

  // The deadline passes. The row turns failed in the navigator, and focus does
  // not move: the user stays on what they were reading.
  await expect(
    page.getByRole('status', { name: 'Failed', exact: true }),
  ).toHaveCount(1, { timeout: 15_000 });
  await expect(page.getByTestId('failed-session-pane')).toHaveCount(0);
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  await expect(page.getByText('first-send hello there').first()).toBeVisible();

  // Opening it shows the failure in its own right: the watchdog observed only
  // silence, so the pane says as much rather than leaving the question hanging,
  // and the prompt that never went out is there with it.
  await cardWithStatus(page, 'Failed').getByTestId('session-node').click();
  await expect(page.getByTestId('failed-session-pane')).toBeVisible();
  await expect(page.getByTestId('failed-session-reason')).toContainText(
    /did not hear why/i,
  );
  await expect(page.getByTestId('failed-session-prompt')).toHaveText(
    'never-ready hang at launch',
  );
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

  // Remove takes it off the list for good.
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(
    page.getByRole('status', { name: 'Failed', exact: true }),
  ).toHaveCount(0);
  await expect.poll(() => stillListed(page, doomed.id)).toBe(false);
});

test('the failure lands on the screen you are already on, and Retry starts the same launch again', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'never-ready hang at launch');
  const doomed = await sessionWithStatus(page, 'spawning');

  // The user is on the failing session's screen — the workspace put them there
  // when its send was accepted. The failure is shown in place; nothing
  // teleports them to the new-session screen.
  await expect(page.getByTestId('failed-session-pane')).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  await expect(page.getByTestId('failed-session-prompt')).toHaveText(
    'never-ready hang at launch',
  );

  // Retry re-attempts the identical launch: the same first prompt in the same
  // working directory (the launch options ride in the same request the composer
  // builds — asserted in `FailedSessionPane.test.tsx`, where the request body is
  // observable), and this row does not linger beside the session that replaced
  // it.
  await page.getByRole('button', { name: 'Retry' }).click();
  const retried = await sessionWithStatus(page, 'spawning');
  expect(retried.id).not.toBe(doomed.id);
  expect(retried.cwd).toBe(doomed.cwd);
  await expect.poll(() => stillListed(page, doomed.id)).toBe(false);

  // Clean up: the retry hangs the same way, so remove it once it gives up
  // rather than leaving a failed row behind for the specs that follow.
  await expect(
    page.getByRole('status', { name: 'Failed', exact: true }),
  ).toHaveCount(1, { timeout: 15_000 });
  await cardWithStatus(page, 'Failed').getByTestId('session-node').click();
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(
    page.getByRole('status', { name: 'Failed', exact: true }),
  ).toHaveCount(0);
});
