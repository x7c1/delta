import { test, expect } from '@playwright/test';
import { TOTAL_SEEDED_SESSIONS } from '@delta/api-mocks';
import { useManualEventControl } from './support/app';

/**
 * Multi-session navigator structure and the focus / closed=view model.
 *
 * The mock fixtures seed two detailed sessions in distinct states — `sess-mock-1`
 * (open, with a thread tree) and `sess-mock-2` (closed) — plus enough filler
 * sessions to span many pages and overflow a single viewport. These specs assert
 * the structural behavior of the session-centric UI — the cursor-paginated,
 * DOM-windowed session list with scroll-to-load, the per-session open/closed
 * status dot, focusing a closed session into a read-only transcript, and the
 * new-session optimistic send — not appearance.
 */

test('the navigator paginates to the end while keeping the rendered DOM windowed', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The list is cursor-paginated (mock page size 2) and DOM-windowed: only the
  // rows in the scroll viewport (+overscan) are mounted. Pagination is driven by
  // the virtualizer's range — when the window reaches the last loaded row the
  // next page is fetched. Repeatedly scrolling the navigator body to its bottom
  // walks the whole list: each scroll reveals the tail, which triggers the next
  // fetch, until `next_cursor` reaches null.
  const sessionNodes = page.getByTestId('session-node');
  const scrollBody = page.getByTestId('sessions-list').locator('..');
  await expect(sessionNodes.first()).toBeVisible();

  // Track how many rows are mounted at once. Because the list is windowed, this
  // peak must stay well below the total once the list is long — that is the
  // proof that off-screen nodes are recycled rather than accumulated.
  let peakRendered = 0;
  let lastCursorReached = -1;
  for (let i = 0; i < 40; i += 1) {
    const mounted = await sessionNodes.count();
    peakRendered = Math.max(peakRendered, mounted);
    // The highest loaded index currently reachable, read from the last mounted
    // row's data-index — it climbs as pagination advances.
    const maxIndex = await page
      .getByTestId('session-node')
      .last()
      .evaluate((el) => Number(el.closest('[data-index]')?.getAttribute('data-index') ?? '-1'));
    if (maxIndex >= TOTAL_SEEDED_SESSIONS - 1) {
      break;
    }
    if (maxIndex === lastCursorReached) {
      // No progress this round; give the in-flight fetch a moment, then retry.
      await page.waitForTimeout(150);
    }
    lastCursorReached = maxIndex;
    await scrollBody.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await page.waitForTimeout(120);
  }

  // The last seeded session is reachable (pagination walked to the end), and the
  // virtual spacer is tall enough to hold every loaded row even though only a
  // window of them is mounted.
  await scrollBody.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await expect(async () => {
    const maxIndex = await page
      .getByTestId('session-node')
      .last()
      .evaluate((el) =>
        Number(el.closest('[data-index]')?.getAttribute('data-index') ?? '-1'),
      );
    expect(maxIndex).toBe(TOTAL_SEEDED_SESSIONS - 1);
  }).toPass();

  // Windowing guarantee: across the whole walk the DOM never held all sessions
  // at once — the peak mounted count stayed comfortably below the total. (If the
  // list were not windowed, every loaded row would stay in the DOM and this peak
  // would equal the total.)
  expect(peakRendered).toBeLessThan(TOTAL_SEEDED_SESSIONS);
});

test('a visible non-focused session shows its sub-thread tree expanded without a click', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The open session ("sess-mock-1") is auto-focused; the closed session
  // ("scratch notes") is on page 1 and visible but NOT focused. Every visible
  // row fetches its own thread tree and renders it expanded by default, so the
  // non-focused session's sub-thread ("scratch ideas") is on screen with no
  // interaction.
  const scratchRow = page
    .getByTestId('session-node')
    .filter({ hasText: 'scratch notes' });
  await expect(scratchRow).toBeVisible();
  const subThread = page.getByRole('button', { name: /scratch ideas/ });
  await expect(subThread).toBeVisible();

  // The transcript still shows the focused (open) session, confirming the tree
  // is shown for navigation without stealing focus.
  await expect(page.getByText('What is a delta?')).toBeVisible();

  // Clicking the non-focused session's sub-thread focuses that session and
  // switches the center pane to the sub-thread.
  await subThread.click();
  const current = page.locator('[aria-current="page"]');
  await expect(current).toHaveText('scratch ideas');
  await expect(page.getByText('Jot down a few ideas for later.')).toBeVisible();
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

  // The windowed list keeps fetching pages to fill the viewport after the first
  // page lands; let that settle before interacting so the open session's kebab
  // is not detached by a row remount mid-click.
  await page.waitForLoadState('networkidle');

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
