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

  // Drill into the child branch via its in-transcript chip. The chip's visible
  // label is just the title, but its accessible name is "Enter <title>", which
  // distinguishes it from the navigator's tree node that also names the branch.
  await page.getByRole('button', { name: /enter delta etymology/i }).click();

  // The pane is now the branch trunk: breadcrumb location and its messages.
  await expect(current).toHaveText('delta etymology');
  await expect(
    page.getByText('Where does the word delta come from?'),
  ).toBeVisible();
});

/**
 * Sending a branch from selected text switches the pane to the freshly-created
 * child thread — it must not revert to main.
 *
 * Selecting a range in a message sets the branch origin; the next Send creates a
 * new child thread server-side and the pane drills into it. This regresses a bug
 * where the send refreshed only the session list (not the focused session's
 * thread tree), so the new child was absent from the cached threads and the
 * workspace reconciled the active thread back to main.
 */
test('sending a branch from selected text switches the pane to the new branch', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  const current = page.locator('[aria-current="page"]');
  await expect(current).toHaveText('main');

  // Select a message's text and release the mouse over it, as a user
  // highlighting a passage would — MessageItem reads window.getSelection() on
  // mouseup to set the branch origin.
  const message = page.locator('[data-testid="message-item"]').first();
  await message.evaluate((article) => {
    const content = article.querySelector('[class*="space-y"]') ?? article;
    const range = document.createRange();
    range.selectNodeContents(content);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    content.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
  });

  // The composer now shows the branch banner.
  await expect(page.getByText('from selected text')).toBeVisible();

  // Send the branch follow-up; the pane drills into the new child thread.
  await page.getByRole('textbox').fill('a follow-up on the selected passage');
  await page.getByRole('button', { name: 'Send' }).click();

  await expect(current).not.toHaveText('main');
  await expect(current).toHaveText('new branch');
});
