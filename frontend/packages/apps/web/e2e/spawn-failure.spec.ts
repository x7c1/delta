import { test, expect } from '@playwright/test';
import { mockSpawnSessionId } from '@delta/api-mocks';
import { emitEvent, useManualEventControl } from './support/app';

/**
 * A new session whose launch never comes up keeps its row and shows the failure
 * on its own screen.
 *
 * The session row is created eagerly, so `POST /api/sends` returns its real id
 * and the workspace switches to the starting session right away. The backend's
 * watchdog then reaps the spawn that never bound and emits `spawn_failed`
 * carrying that same id — marking the row `failed` rather than deleting it. So
 * the screen the user is already on simply changes what it shows: why the
 * launch did not start, the prompt that was never delivered, and the two things
 * to do about it. Nothing teleports the user anywhere, and nothing has to be
 * rescued into the composer, because nothing was deleted.
 */
test('a failed spawn shows its failure in place, with Retry and Remove', async ({
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
  // body to travel in — it reaches the user on the row instead, which is where
  // the failed session's screen reads it from.
  await emitEvent(page, {
    kind: 'spawn_failed',
    cancelled: false,
    session_id: mockSpawnSessionId(1),
    pane_token: 'pane-never-bound',
    reason: 'git error: invalid reference: origin/nope',
  });

  // The user stays exactly where they were; that screen now explains itself.
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  const pane = page.getByTestId('failed-session-pane');
  await expect(pane).toBeVisible();
  await expect(page.getByTestId('failed-session-reason')).toContainText(
    'invalid reference: origin/nope',
  );
  // The prompt that never went out is still a real row on the server, read
  // back here rather than carried across in the browser's memory.
  await expect(page.getByTestId('failed-session-prompt')).toHaveText(
    'start something that never boots',
  );
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

  // Remove takes the failed session off the list, and focus lands on whatever
  // is left rather than on a screen describing a session that is gone.
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(pane).toHaveCount(0);
});
