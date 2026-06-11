import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * The first send's pending chip survives the spawn transition.
 *
 * A new-session send starts as an optimistic pending item keyed to the
 * not-yet-existing session. When the spawn registers (the fake's hooks bind
 * it), focus jumps from the new-session state to the real session — and the
 * chip must survive that re-keying and stay visible until the scripted turn
 * actually completes, not vanish at the focus transition.
 *
 * Scenario `first-send`: the fake holds the turn open for a scripted beat
 * after the prompt, then replies and fires `Stop`.
 */
test('the first-send pending chip stays visible until the scripted turn completes', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'first-send hold then answer');

  // Optimistically pending immediately after Send.
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);

  // The spawn registers and focus switches from the new-session state to the
  // real session (the new-session placeholder leaves). The chip must still be
  // there — the turn has not completed.
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  await expect(pending).toHaveCount(1);

  // The user's own message renders from the transcript while the turn is
  // still in flight; the chip still must not drain early.
  await expect(page.getByTestId('message-item')).toHaveCount(1);
  await expect(pending).toHaveCount(1);

  // The scripted turn completes: the assistant reply lands and the chip drains.
  await expect(page.getByTestId('message-item')).toHaveCount(2);
  await expect(pending).toHaveCount(0);
});
