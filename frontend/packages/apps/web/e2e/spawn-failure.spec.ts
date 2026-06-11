import { test, expect } from '@playwright/test';
import { emitEvent, useManualEventControl } from './support/app';

/**
 * A new session whose launch never comes up surfaces a recoverable failure.
 *
 * The user starts a new session (the optimistic "pending" chip appears), then the
 * backend's watchdog reaps the spawn that never bound and emits `spawn_failed`.
 * The chip must stop looking stuck: it turns into a distinct error row offering
 * Retry and Dismiss. Dismiss clears it.
 */
test('a failed spawn turns the pending chip into a Retry / Dismiss error row', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Enter the new-session composer state and choose a directory (mandatory
  // before the first message can be sent), then send the first message.
  await page.getByRole('button', { name: 'New', exact: true }).click();
  await page.getByTestId('workdir-use-current').click();
  await page.getByTestId('workdir-confirm').click();
  await expect(page.getByTestId('new-session-empty')).toBeVisible();

  await page.getByRole('textbox').fill('start something that never boots');
  await page.getByRole('button', { name: 'Send' }).click();

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);

  // The spawn never bound; the backend reaps it and emits spawn_failed. The
  // event's ids cannot be correlated to the still-unbound pending, so the oldest
  // unbound new-session pending is the one marked failed.
  await emitEvent(page, {
    kind: 'spawn_failed',
    session_id: 'sess-never-bound',
    pane_token: 'pane-never-bound',
  });

  await expect(pending).toContainText(/failed to start/i);
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
  const dismiss = page.getByRole('button', { name: 'Dismiss' });
  await expect(dismiss).toBeVisible();

  // Dismiss clears the failed chip.
  await dismiss.click();
  await expect(pending).toHaveCount(0);
});
