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
 * reads `Starting`, its composer says a message sent now waits for the session,
 * and its first prompt sits in the pending strip.
 *
 * And a message composed inside that window really is accepted: Send stays
 * live, the server records the message as a `queued` row (the strip says so),
 * and once the launch binds and the opening turn ends it is typed and answered
 * — without the user ever having watched a disabled button. When the hook
 * finally lands the same session simply becomes `Open`: one continuous session,
 * no hand-off the user can see.
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
    'Message sends when the session is ready…',
  );
  // A starting session has no live pane, so it reaches the pane read-only —
  // but it was never closed, and no send resumes it. The closed notice must
  // stay off it, or it would contradict the placeholder.
  await expect(page.getByTestId('readonly-notice')).toHaveCount(0);

  // The first prompt is visible exactly once while it waits.
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('slow-start wake up eventually');

  // A follow-up CAN be sent inside the window: the server accepts it as a
  // `queued` row and types it when the launch binds, so Send stays live.
  await textbox.fill('a follow-up, once you are up');
  const send = page.getByRole('button', { name: 'Send' });
  await expect(send).toBeEnabled();
  await send.click();

  // It is accepted, not dispatched: the strip now holds both messages, and the
  // second one says what it is waiting for.
  await expect(pending).toHaveCount(2);
  await expect(pending.nth(1)).toContainText('a follow-up, once you are up');
  await expect(
    page.getByText('queued — sends when the session starts'),
  ).toBeVisible();
  // The draft was consumed by the accepted send, not held back.
  await expect(textbox).toHaveValue('');

  // The hook lands: the very same card flips to Open — no second session, no
  // return to the new-session screen — and the scripted reply arrives.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1, { timeout: 15_000 });
  await expect(page.getByRole('status', { name: 'Starting', exact: true })).toHaveCount(0);
  // The opening turn ends, which is what flushes the queue: the follow-up is
  // typed and answered in its own turn. Four messages — both prompts and both
  // scripted answers — asserted by count, so the spec never depends on what the
  // scenario replies (see e2e.md).
  await expect(page.getByTestId('message-item')).toHaveCount(4, {
    timeout: 15_000,
  });
  await expect(page.getByText('a follow-up, once you are up')).toBeVisible();
  // Nothing is left waiting, and the composer is live again.
  await expect(pending).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByRole('button', { name: 'Send' })).toBeDisabled();
});
