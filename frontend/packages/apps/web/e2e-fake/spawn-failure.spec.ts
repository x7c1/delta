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

  // Dismiss clears it; nothing else of the failed spawn remains.
  await page.getByRole('button', { name: 'Dismiss' }).click();
  await expect(pending).toHaveCount(0);
});
