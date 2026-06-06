import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * Drilling into a child branch switches the right pane to that branch's trunk.
 *
 * The seed data has a child thread ("delta etymology") sprouting from an
 * assistant message in main, surfaced as a branch chip. Clicking the chip makes
 * the branch the active thread: the breadcrumb's current location becomes the
 * branch and the pane shows the branch's own messages.
 */
test('clicking a branch chip switches the pane to the branch trunk', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Start on main: its breadcrumb current location is "main".
  const current = page.locator('[aria-current="page"]');
  await expect(current).toHaveText('main');

  // Drill into the child branch via its in-transcript chip (the "[enter →]"
  // affordance distinguishes the chip from the navigator's tree node, which
  // also names the branch).
  await page.getByRole('button', { name: /delta etymology.*enter/ }).click();

  // The pane is now the branch trunk: breadcrumb location and its messages.
  await expect(current).toHaveText('delta etymology');
  await expect(
    page.getByText('Where does the word delta come from?'),
  ).toBeVisible();
});
