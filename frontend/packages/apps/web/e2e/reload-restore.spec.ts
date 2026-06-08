import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * After a reload the previously active branch is restored from localStorage.
 *
 * The nav store persists the active thread, so drilling into a branch and then
 * reloading must land back on that branch — its content is still present — not
 * snap back to main. This exercises the localStorage layout restore.
 */
test('branch content survives a reload via persisted layout', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The branch chip's accessible name is "Enter <title>" (its visible label is
  // just the title), distinct from the navigator tree node of the same branch.
  await page.getByRole('button', { name: /enter delta etymology/i }).click();
  const current = page.locator('[aria-current="page"]');
  await expect(current).toHaveText('delta etymology');

  await page.reload();

  // Restored to the branch, not reset to main.
  await expect(current).toHaveText('delta etymology');
  await expect(
    page.getByText('Where does the word delta come from?'),
  ).toBeVisible();
});
