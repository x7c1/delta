import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * A launch that never becomes ready surfaces as a recoverable failure.
 *
 * Scenario `never-ready`: the fake skips its `SessionStart` hook and hangs, so
 * the spawn never binds. The backend's launch watchdog (its deadline shrunk
 * via DELTA_LAUNCH_DEADLINE_MS by the suite's server script) reaps the spawn,
 * kills its pane, and emits `spawn_failed` — and the optimistic pending chip
 * must turn into an error row offering Retry and Dismiss instead of looking
 * stuck forever.
 */
test('a spawn that never binds turns the pending chip into a Retry / Dismiss row', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'never-ready hang at launch');

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);

  // The watchdog reaps the spawn after the (shortened) deadline; the chip
  // becomes an explicit failure row. The timeout spans the deadline plus the
  // watchdog tick with margin.
  await expect(pending).toContainText(/failed to start/i, { timeout: 15_000 });
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

  // Dismiss clears it; nothing else of the failed spawn remains.
  await page.getByRole('button', { name: 'Dismiss' }).click();
  await expect(pending).toHaveCount(0);
});
