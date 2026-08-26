import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * A new session is the user's the moment the server accepts its first send —
 * not when its launch finishes coming up.
 *
 * Scenario `slow-start`: the fake holds its `SessionStart` hook back by 2.5 s,
 * stretching the window between "the POST returned" and "the session
 * registered" wide enough to observe. Inside that window the workspace must
 * already be ON the new session — the row is listed as `spawning`, so its card
 * reads `Starting`, its composer says the session is starting and offers no
 * send, and its first prompt sits in the pending strip. When the hook finally
 * lands the same session simply becomes `Open` and the scripted reply arrives:
 * one continuous session, no hand-off the user can see.
 */
test('a slow launch is focused as a starting session, then comes up in place', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'slow-start wake up eventually');

  // Inside the delayed-hook window. Each assertion below must hold BEFORE the
  // fake fires `SessionStart`, so they are kept tight and ordered cheapest
  // first; the scenario's 2.5 s is the budget for all of them.
  const startingCard = page
    .locator('li')
    .filter({ has: page.getByRole('status', { name: 'Starting', exact: true }) });
  await expect(startingCard).toHaveCount(1, { timeout: 2_000 });
  // The new-session screen is behind us: this is the spawned session's screen.
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  const textbox = page.getByRole('textbox');
  await expect(textbox).toHaveAttribute(
    'placeholder',
    'This session is starting…',
  );
  // A starting session has no live pane, so it reaches the pane read-only —
  // but it was never closed, and no send resumes it. The closed notice must
  // stay off it, or it would contradict the placeholder.
  await expect(page.getByTestId('readonly-notice')).toHaveCount(0);

  // The first prompt is visible exactly once while it waits.
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('slow-start wake up eventually');

  // A follow-up cannot be sent yet: the server would refuse it
  // (`409 session_spawning`), so the composer does not offer one — even with a
  // draft ready to go.
  await textbox.fill('a follow-up, once you are up');
  await expect(page.getByRole('button', { name: 'Send' })).toBeDisabled();

  // The hook lands: the very same card flips to Open — no second session, no
  // return to the new-session screen — and the scripted reply arrives.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1, { timeout: 15_000 });
  await expect(page.getByRole('status', { name: 'Starting', exact: true })).toHaveCount(0);
  // The prompt and the scripted answer, in that order — asserted by count, so
  // the spec never depends on what the scenario replies (see e2e.md).
  await expect(page.getByTestId('message-item')).toHaveCount(2, {
    timeout: 15_000,
  });
  // And the composer is live again, with the draft that was held back ready to
  // send — the same textarea, never reset by the hand-off.
  await expect(textbox).toHaveValue('a follow-up, once you are up');
  await expect(page.getByRole('button', { name: 'Send' })).toBeEnabled();
});
