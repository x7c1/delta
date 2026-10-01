import type { SessionsResponse } from '@delta/wire-gen';
import { test, expect, type Page } from './support/fixtures';
import { shownTerminal, startNewSession } from './support/app';

/**
 * A launch that never binds becomes an ordinary session the user can open,
 * read, retry and remove.
 *
 * Scenario `never-ready`: the fake skips its `SessionStart` hook and hangs, so
 * the spawn never binds and the backend's launch watchdog (its deadline shrunk
 * via DELTA_LAUNCH_DEADLINE_MS by the suite's server script) captures its pane,
 * kills it and emits `spawn_failed`.
 *
 * What the server does with the row is the whole point: it KEEPS it, marked
 * `failed`, with the prompt that was never delivered still open against it. So
 * the failure has a place of its own — a screen that says why it did not start
 * and offers Retry and Remove — instead of a row that vanishes and forces every
 * surface around it to compensate for the absence.
 *
 * Whether anyone is looking decides whether the watchdog acts at all. A pane
 * with a PTY bridge attached is never reaped — somebody may be answering the
 * prompt the launch stopped on — and a new session's terminal is open by
 * default on this (large) layout, so a hung launch is watched for as long as
 * its screen is focused with the terminal open. The browser holds a starting
 * pane's bridge only while that session is focused, so moving away, or closing
 * its terminal, lets the deadline run again from the detach.
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
  // holding focus when the deadline passes. Its terminal opens with it, but the
  // bridge is dropped the moment focus moves on, so nothing keeps it watched.
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
  // not move: the user stays on what they were reading. A failed card is listed
  // after every open one, and earlier specs leave many open, but a launch this
  // page started is pinned to the top of the navigator — so it is in view
  // without scrolling.
  await expect(cardWithStatus(page, 'Failed')).toBeInViewport({
    timeout: 15_000,
  });
  await expect(page.getByTestId('failed-session-pane')).toHaveCount(0);
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  await expect(page.getByText('first-send hello there').first()).toBeVisible();

  // Opening it shows the failure in its own right. The watchdog heard nothing
  // from the launch, but that is not the same as knowing nothing: it names the
  // deadline the launch missed, and quotes what the pane was showing when it
  // gave up — read just before the pane was killed, which is the only moment
  // that evidence exists. The prompt that never went out is there with it.
  await cardWithStatus(page, 'Failed').getByTestId('session-node').click();
  await expect(page.getByTestId('failed-session-pane')).toBeVisible();
  const reason = page.getByTestId('failed-session-reason');
  await expect(reason).toContainText(/did not start within \d+ seconds?/i);
  await expect(reason).toContainText('fake-claude session');
  await expect(reason).not.toContainText(/did not hear why/i);
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
  // when its send was accepted — but has closed its terminal, so nobody is
  // watching the pane and the watchdog is free to give up on it. (With the
  // terminal open the launch is deliberately left alone: see the next case.)
  await page.getByRole('button', { name: 'Close terminal' }).click();

  // The failure is shown in place; nothing teleports them to the new-session
  // screen.
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
  // rather than leaving a failed row behind for the specs that follow. It is a
  // new session, with its terminal open by default, so close that too or the
  // launch is watched and never gives up.
  await expect(page.getByTestId('failed-session-pane')).toHaveCount(0);
  await page.getByRole('button', { name: 'Close terminal' }).click();
  await expect(cardWithStatus(page, 'Failed')).toBeInViewport({
    timeout: 15_000,
  });
  await cardWithStatus(page, 'Failed').getByTestId('session-node').click();
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(
    page.getByRole('status', { name: 'Failed', exact: true }),
  ).toHaveCount(0);
});

test('a launch you are watching in its terminal is left alone past the deadline, and Close cancels it', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'never-ready hang at launch');
  const doomed = await sessionWithStatus(page, 'spawning');

  // The terminal is open on the starting session without a click, and attached:
  // tmux redraws the pane into it, banner included. This is the window in which
  // a real launch may be sitting on a prompt only a human can answer.
  const rows = shownTerminal(page).locator('.xterm-rows');
  await expect(rows).toContainText('fake-claude session', { timeout: 15_000 });

  // Several times the suite's launch deadline (DELTA_LAUNCH_DEADLINE_MS, 3 s in
  // `support/server.ts`) passes. There is no event to wait for — the point is
  // that none arrives — so this is a plain wait.
  await page.waitForTimeout(9_000);

  // Still starting, still attachable, and never described as failed: the pane
  // is being watched, so the watchdog does not take it away from the user.
  await expect(
    page.getByRole('status', { name: 'Starting', exact: true }),
  ).toHaveCount(1);
  await expect(
    page.getByRole('status', { name: 'Failed', exact: true }),
  ).toHaveCount(0);
  await expect(page.getByTestId('failed-session-pane')).toHaveCount(0);
  await expect(
    shownTerminal(page).locator('.xterm-helper-textarea'),
  ).toBeAttached();
  await expect(rows).toContainText('fake-claude session');
  expect((await sessionWithStatus(page, 'spawning')).id).toBe(doomed.id);

  // Giving up is the user's call, and it is a close like any other: the card's
  // kebab Close cancels the launch, which is worded as a cancel, not a failure.
  await cardWithStatus(page, 'Starting')
    .getByRole('button', { name: /^Session actions for/ })
    .click();
  await page.getByRole('menuitem', { name: 'Close' }).click();
  await expect(page.getByTestId('failed-session-pane')).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByText('This launch was cancelled.')).toBeVisible();

  // Clean up the cancelled row for the specs that follow.
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect.poll(() => stillListed(page, doomed.id)).toBe(false);
});

test('a launch you watched and then left for another session turns failed at the deadline', async ({
  page,
}) => {
  await page.goto('/');

  // A bound session to switch to, started first.
  const earlier = new Set(
    (await listSessions(page)).map((item) => item.session.id),
  );
  await startNewSession(page, 'first-send hello there');
  await expect(page.getByText('first-send hello there').first()).toBeVisible({
    timeout: 15_000,
  });
  let boundId: string | undefined;
  await expect
    .poll(async () => {
      boundId = (await listSessions(page)).find(
        (item) => !earlier.has(item.session.id) && item.open,
      )?.session.id;
      return boundId !== undefined;
    })
    .toBe(true);

  // The doomed launch, watched: its terminal is open and attached.
  await startNewSession(page, 'never-ready hang at launch');
  const doomed = await sessionWithStatus(page, 'spawning');
  await expect(shownTerminal(page).locator('.xterm-rows')).toContainText(
    'fake-claude session',
    { timeout: 15_000 },
  );

  // Picking the other session keeps the terminal column mounted, and a bound
  // session's bridge would be kept alive, hidden, right here. A starting one is
  // not: it is dropped, so nobody is watching the pane any more, and the
  // deadline runs out from the detach.
  // Sessions left open by earlier specs share this server. The navigator lists
  // them in the server's order, newest-active first, so the first open card is
  // the one just started — checked here rather than assumed.
  expect(
    (await listSessions(page)).find((item) => item.open)
      ?.session.id,
  ).toBe(boundId);
  await cardWithStatus(page, 'Open').first().getByTestId('session-node').click();
  await expect(cardWithStatus(page, 'Failed')).toBeInViewport({
    timeout: 15_000,
  });

  await cardWithStatus(page, 'Failed').getByTestId('session-node').click();
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect.poll(() => stillListed(page, doomed.id)).toBe(false);
});
