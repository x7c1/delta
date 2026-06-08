import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * Multi-session navigator structure and the focus / closed=view model.
 *
 * The mock fixtures seed two sessions in distinct states: `sess-mock-1` (open,
 * with a thread tree) and `sess-mock-2` (closed). These specs assert the
 * structural behavior of the session-centric UI — the session list, the
 * open-session count and open/closed indicator, focusing a closed session into
 * a read-only transcript, and the new-session optimistic send — not appearance.
 */

test('the navigator lists every session with an open count', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Both seeded sessions appear as top-level nodes.
  await expect(page.getByTestId('session-node')).toHaveCount(2);
  // Exactly one of the two is open.
  await expect(page.getByTestId('open-session-count')).toHaveText('open: 1');
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

test('closing an open session via its kebab menu drops the open count', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await expect(page.getByTestId('open-session-count')).toHaveText('open: 1');

  // The open session's actions menu is enabled; open it and select Close.
  await page
    .getByRole('button', { name: /^Session actions for/ })
    .and(page.locator(':not([disabled])'))
    .click();
  await page.getByRole('menuitem', { name: 'Close' }).click();

  // The mock flips the session closed and the refetched list reflects it.
  await expect(page.getByTestId('open-session-count')).toHaveText('open: 0');
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
