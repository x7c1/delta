import { test, expect } from '@playwright/test';
import { mockSpawnSessionId } from '@delta/api-mocks';
import { emitEvent, useManualEventControl } from './support/app';

/**
 * A new session whose launch never comes up surfaces a recoverable failure.
 *
 * The user starts a new session (the pending chip appears). The session row is
 * created eagerly, so `POST /api/sends` returned its real id; the backend's
 * watchdog then reaps the spawn that never bound and emits `spawn_failed`
 * carrying that same id. The chip must stop looking stuck: it turns into a
 * distinct error row offering Retry and Dismiss. Dismiss clears it.
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

  // The spawn never bound; the backend reaps it and emits spawn_failed with
  // the REAL session id the POST response carried (the mock mints
  // deterministic spawn ids, so the first spawn's id is known here).
  await emitEvent(page, {
    kind: 'spawn_failed',
    session_id: mockSpawnSessionId(1),
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
