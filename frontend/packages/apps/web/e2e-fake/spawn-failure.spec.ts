import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * A launch that never becomes ready surfaces as a recoverable failure.
 *
 * Scenario `never-ready`: the fake skips its `SessionStart` hook and hangs, so
 * the spawn never binds. The session is nevertheless the user's from the moment
 * the POST is accepted — they are moved onto it and watch it start — which is
 * what makes this failure a hand-off in reverse: the backend's launch watchdog
 * (its deadline shrunk via DELTA_LAUNCH_DEADLINE_MS by the suite's server
 * script) reaps the spawn, kills its pane, and emits `spawn_failed`, deleting
 * the row on screen. The user must land back on the new-session screen with an
 * error row offering Retry and Dismiss, rather than be left looking at a
 * session that no longer exists.
 *
 * And nothing they typed may be lost with the row. A session accepts sends as
 * `queued` rows for as long as it is starting, and those rows cascade away with
 * the session — so the failure event carries their text, and the browser puts
 * everything the Retry chip does not already hold back into the new-session
 * composer. Restored, never re-sent: the message waits there for the user.
 */
test('a spawn that never binds is focused first, then hands back a Retry / Dismiss row', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'never-ready hang at launch');

  // The accepted session takes focus right away, still starting.
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  await expect(
    page.getByRole('status', { name: 'Starting', exact: true }),
  ).toHaveCount(1);
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);

  // A second message, composed while the launch was still coming up: accepted
  // as a `queued` row that never reaches an agent.
  const textbox = page.getByRole('textbox');
  await textbox.fill('typed while it was starting');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(pending).toHaveCount(2);

  // The watchdog reaps the spawn after the (shortened) deadline. Its row is
  // deleted, so focus returns to the new-session screen — where the chip is
  // now an explicit failure row. The timeout spans the deadline plus the
  // watchdog tick with margin.
  await expect(page.getByTestId('new-session-empty')).toBeVisible({
    timeout: 15_000,
  });
  await expect(pending).toContainText(/failed to start/i, { timeout: 15_000 });
  // The watchdog observes only silence, so the card carries no reason line —
  // unlike a launch preparation that failed with a git or tmux error.
  await expect(page.getByTestId('pending-fail-reason')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(
    page.getByRole('status', { name: 'Starting', exact: true }),
  ).toHaveCount(0);

  // The second message's rows are gone with the session, so its text came back
  // on the failure event and is waiting in the new-session composer. The first
  // prompt is NOT duplicated there — the Retry button above is what re-sends
  // that one — and neither message was re-sent behind the user's back.
  await expect(page.getByRole('textbox')).toHaveValue(
    'typed while it was starting',
  );
  // The composer is a different surface from the chip, so the chip is what says
  // the message went there — and that Retry will not take it along.
  await expect(page.getByTestId('pending-fail-note')).toHaveText(
    '1 later message was returned to the composer. Retry re-sends only this one.',
  );

  // Dismiss clears the failure row; nothing else of the failed spawn remains,
  // and the restored draft is untouched by it.
  await page.getByRole('button', { name: 'Dismiss' }).click();
  await expect(pending).toHaveCount(0);
  await expect(page.getByRole('textbox')).toHaveValue(
    'typed while it was starting',
  );
});
