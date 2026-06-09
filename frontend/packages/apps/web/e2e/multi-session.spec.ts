import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * Multi-session navigator structure and the focus / closed=view model.
 *
 * The mock fixtures seed two detailed sessions in distinct states — `sess-mock-1`
 * (open, with a thread tree) and `sess-mock-2` (closed) — plus filler sessions
 * that push the list past one page. These specs assert the structural behavior
 * of the session-centric UI — the cursor-paginated session list with
 * scroll-to-load, the per-session open/closed status dot, focusing a closed
 * session into a read-only transcript, and the new-session optimistic send —
 * not appearance.
 */

test('the navigator paginates the session list to the end via the scroll sentinel', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The list is cursor-paginated (mock page size 2). The bottom sentinel drives
  // loading: while more pages remain and the sentinel is within the scroll
  // viewport, the IntersectionObserver keeps fetching the next page. With a
  // short list the sentinel stays visible, so every page loads without an
  // explicit scroll; a longer list would load incrementally as the user scrolls.
  // Either way the chain ends when `next_cursor` reaches null and the sentinel
  // unmounts.
  const sentinel = page.getByTestId('sessions-load-more-sentinel');
  for (let i = 0; i < 6 && (await sentinel.count()) > 0; i += 1) {
    await sentinel.scrollIntoViewIfNeeded();
    await page.waitForTimeout(150);
  }

  // All six seeded sessions (two detailed + four filler) are now present and the
  // sentinel is gone (next_cursor reached null), confirming pagination walked to
  // the end without dropping or duplicating any page.
  await expect(page.getByTestId('session-node')).toHaveCount(6);
  await expect(sentinel).toHaveCount(0);
  // Exactly one session is open, shown by its status dot.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1);
});

test('focusing a closed session shows its transcript read-only', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Focus the closed session ("scratch notes") by its label; the kebab menu
  // shares the label, so target the row by test id.
  await page
    .getByTestId('session-node')
    .filter({ hasText: 'scratch notes' })
    .click();

  // Its transcript renders, but with a read-only notice (closed session).
  await expect(page.getByTestId('readonly-notice')).toBeVisible();
  await expect(
    page.getByText('Remind me what scratch is for.'),
  ).toBeVisible();
});

test('a closed session resumes after a Send via the pending queue', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page
    .getByTestId('session-node')
    .filter({ hasText: 'scratch notes' })
    .click();
  await expect(page.getByTestId('readonly-notice')).toBeVisible();

  // Sending to a closed session resumes it; the send is surfaced optimistically
  // in the pending queue rather than via a navigator badge.
  await page.getByRole('textbox').fill('pick this back up');
  await page.getByRole('button', { name: 'Send' }).click();

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('pick this back up');
});

test('closing an open session via its kebab menu clears its open dot', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1);

  // The open session's actions menu is enabled; open it and select Close.
  await page
    .getByRole('button', { name: /^Session actions for/ })
    .and(page.locator(':not([disabled])'))
    .click();
  await page.getByRole('menuitem', { name: 'Close' }).click();

  // The mock flips the session closed and the refetched list reflects it: no
  // session carries the "Open" status dot any more.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(0);
});

test('starting a new session shows the optimistic send', async ({ page }) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Enter the new-session composer state.
  await page.getByRole('button', { name: 'New', exact: true }).click();
  await expect(page.getByTestId('new-session-empty')).toBeVisible();

  // The first Send is shown optimistically in the pending queue.
  await page.getByRole('textbox').fill('hello new session');
  await page.getByRole('button', { name: 'Send' }).click();

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('hello new session');
});
