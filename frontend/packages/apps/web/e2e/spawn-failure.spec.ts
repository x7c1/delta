import { test, expect } from '@playwright/test';
import { mockSpawnSessionId } from '@delta/api-mocks';
import { emitEvent, useManualEventControl } from './support/app';

/**
 * A new session whose launch never comes up surfaces a recoverable failure.
 *
 * The session row is created eagerly, so `POST /api/sends` returns its real id
 * and the workspace switches to the starting session right away. The backend's
 * watchdog then reaps the spawn that never bound and emits `spawn_failed`
 * carrying that same id — deleting the row the user is looking at. The failure
 * therefore has to do two things: take the user back to the new-session screen,
 * and stop the chip looking stuck — it becomes a distinct error row offering
 * Retry and Dismiss. Dismiss clears it.
 */
test('a failed spawn returns to the new-session screen with a Retry / Dismiss row', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Enter the new-session composer state and choose a directory (mandatory
  // before the first message can be sent), then send the first message.
  // Phase B: the Directory tab's inline picker commits on row click — no
  // Select button to chase.
  await page.getByRole('button', { name: 'New session', exact: true }).click();
  await page.getByTestId('new-session-tab-directory').click();
  await page.getByTestId('workdir-use-current').click();
  await expect(page.getByTestId('workdir-chip')).toBeVisible();

  await page.getByRole('textbox').fill('start something that never boots');
  await page.getByRole('button', { name: 'Send' }).click();

  // The workspace focuses the accepted session at once: the new-session screen
  // is gone and the starting session's first prompt is in its pending strip.
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);

  // The launch never came up: the backend emits spawn_failed with the REAL
  // session id the POST response carried (the mock mints deterministic spawn
  // ids, so the first spawn's id is known here). The launch preparation runs
  // after the send is accepted, so the git error that killed it has no response
  // body to travel in: the event's `reason` is the only account of it the user
  // gets, and it has to reach the card.
  await emitEvent(page, {
    kind: 'spawn_failed',
    session_id: mockSpawnSessionId(1),
    pane_token: 'pane-never-bound',
    reason: 'git error: invalid reference: origin/nope',
  });

  // The focused session no longer exists, so focus goes back to the
  // new-session screen — which is where the failure's card lives.
  await expect(page.getByTestId('new-session-empty')).toBeVisible();
  await expect(pending).toContainText(/failed to start/i);
  await expect(page.getByTestId('pending-fail-reason')).toContainText(
    'invalid reference: origin/nope',
  );
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
  const dismiss = page.getByRole('button', { name: 'Dismiss' });
  await expect(dismiss).toBeVisible();

  // Dismiss clears the failed chip.
  await dismiss.click();
  await expect(pending).toHaveCount(0);
});
