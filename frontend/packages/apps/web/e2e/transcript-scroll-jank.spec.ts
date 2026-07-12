import { test, expect, type Page } from '@playwright/test';
import { MOCK_BIG_THREAD_KEY } from '@delta/api-mocks';
import { useManualEventControl } from './support/app';

/**
 * Regression guard for transcript scroll jank when scrolling UP through a long
 * thread of wildly-variable-height messages.
 *
 * The bug: the virtualizer seeds every unmeasured row with one flat height, so
 * the first time a tall history row above the fold is mounted and measured, the
 * large `measured − estimate` delta is reconciled by yanking `scrollTop` back
 * down (the `shouldAdjustScrollPositionOnItemSizeChange` compensation). Each
 * yank throws the user back toward the tail, so a slow upward scroll barely
 * makes progress. The fix is a content-aware `estimateSize` (see
 * `estimateMessageHeight`) that seeds each row close to its real height, so the
 * reconciliation delta — and thus the yank — is small.
 *
 * The metric: scroll up in fixed wheel steps and watch `scrollTop`. Scrolling
 * up, `scrollTop` must only decrease; any upward jerk (a positive step) is a
 * compensation yank. We bound the total magnitude of those jerks and require
 * the scroll to make most of its intended upward progress. The mechanism is a
 * property of the estimate error, not of a specific engine, so it reproduces
 * deterministically here on the mock-mode chromium suite (the field reports are
 * WebKit, where the same reconciliation is more visible under kinetic scroll).
 */

const MESSAGE_COUNT = 200;
const WHEEL_STEP_PX = 60;
const UP_STEPS = 80;
const STEP_WAIT_MS = 45;
const INTENDED_PROGRESS_PX = WHEEL_STEP_PX * UP_STEPS;

async function findScrollTop(page: Page): Promise<number> {
  return page.evaluate(() => {
    let el = document.querySelector(
      '[data-testid="transcript-message-list"]',
    ) as HTMLElement | null;
    while (el && el !== document.body) {
      const oy = getComputedStyle(el).overflowY;
      if (oy === 'auto' || oy === 'scroll') break;
      el = el.parentElement;
    }
    return el ? el.scrollTop : 0;
  });
}

test('scrolling up a long variable-height thread makes smooth progress without scrollTop yank-back', async ({
  page,
}) => {
  await page.addInitScript(
    ([key, count]) => {
      (window as unknown as Record<string, unknown>)[key as string] = count;
    },
    [MOCK_BIG_THREAD_KEY, MESSAGE_COUNT] as const,
  );
  await useManualEventControl(page);
  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto('/');

  await expect(page.getByTestId('message-item').first()).toBeVisible();
  // The app lands at the bottom of the thread; let the initial layout settle.
  await page.waitForTimeout(400);

  const list = page.getByTestId('transcript-message-list');
  const box = await list.boundingBox();
  if (box) {
    await page.mouse.move(box.x + box.width / 2, 400);
  }

  const scrollTops: number[] = [await findScrollTop(page)];
  for (let i = 0; i < UP_STEPS; i += 1) {
    await page.mouse.wheel(0, -WHEEL_STEP_PX); // negative = scroll up
    await page.waitForTimeout(STEP_WAIT_MS);
    scrollTops.push(await findScrollTop(page));
  }

  // Sum the magnitude of upward jerks: while scrolling up, scrollTop should be
  // monotonically non-increasing, so any step where it INCREASES is a
  // compensation yank-back. Before the fix this summed to ~3200px on a
  // ~4800px intended scroll; after it is a couple hundred px at most.
  let yankMagnitude = 0;
  for (let i = 1; i < scrollTops.length; i += 1) {
    const delta = scrollTops[i] - scrollTops[i - 1];
    if (delta > 5) yankMagnitude += delta;
  }
  const netProgress = scrollTops[0] - scrollTops[scrollTops.length - 1];

  console.log(
    `[scroll-jank] netProgress=${Math.round(netProgress)} of ${INTENDED_PROGRESS_PX} intended, yankMagnitude=${Math.round(yankMagnitude)}`,
  );

  // The scroll must reach most of its intended upward distance...
  expect(netProgress).toBeGreaterThan(INTENDED_PROGRESS_PX * 0.8);
  // ...and must not be repeatedly yanked back down. The bound sits an order of
  // magnitude below the pre-fix ~3200px yank, with wide headroom over the
  // post-fix couple-hundred px so it is not flaky.
  expect(yankMagnitude).toBeLessThan(900);
});
