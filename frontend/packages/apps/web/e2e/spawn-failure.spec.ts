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
 * therefore has to do three things: take the user back to the new-session
 * screen, stop the chip looking stuck — it becomes a distinct error row
 * offering Retry and Dismiss — and hand back the messages the launch never
 * delivered. Those `send` rows are deleted with the session, so the event
 * carries their text and the browser restores everything the Retry chip does
 * not already hold into the new-session composer. Dismiss clears the row and
 * leaves the restored draft alone.
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
  // `unsent` carries what the session had accepted and never delivered: its
  // first prompt (send id 1, which the Retry chip already holds) and the
  // message typed after it while the launch was still coming up.
  await emitEvent(page, {
    kind: 'spawn_failed',
    cancelled: false,
    session_id: mockSpawnSessionId(1),
    pane_token: 'pane-never-bound',
    reason: 'git error: invalid reference: origin/nope',
    unsent: [
      { send_id: 1, text: 'start something that never boots' },
      { send_id: 2, text: 'typed while it was starting' },
    ],
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

  // The second message is back in the composer, ready to send again — and the
  // first prompt is NOT duplicated there, because Retry is what re-sends that
  // one. Nothing was re-sent on the user's behalf.
  await expect(page.getByRole('textbox')).toHaveValue(
    'typed while it was starting',
  );
  // The card accounts for it, since the composer it went to is a different
  // surface and Retry does not take it along.
  await expect(page.getByTestId('pending-fail-note')).toHaveText(
    '1 later message was returned to the composer. Retry re-sends only this one.',
  );

  // Dismiss clears the failed chip, leaving the restored draft untouched.
  await dismiss.click();
  await expect(pending).toHaveCount(0);
  await expect(page.getByRole('textbox')).toHaveValue(
    'typed while it was starting',
  );
});
