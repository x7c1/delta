import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The per-message metadata line and its hover popover render in the
 * conversation view.
 *
 * The seeded main thread's messages carry the transcript-derived metadata
 * (model, response time, cwd, git branch). This asserts the structural
 * behavior: the latest assistant message shows the full meta row — the working
 * location (cwd, branch) on the left and the timestamp plus `model in
 * <responseTime>` on the right — and hovering the timestamp reveals a popover
 * listing model, cwd and branch. It never asserts styling or response wording.
 */
test('the latest assistant message shows the full meta row and a hover popover', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The seeded main thread renders its transcript.
  await expect(page.getByTestId('message-item').first()).toBeVisible();

  // The latest assistant message carries the full meta row: the working
  // location (cwd home-collapsed, branch) on the left and the model on the
  // right.
  const latestMeta = page.locator('[data-testid="message-meta"][data-latest="true"]');
  await expect(latestMeta).toHaveCount(1);
  await expect(latestMeta.getByTestId('meta-cwd')).toContainText('~/repo');
  await expect(latestMeta.getByTestId('meta-branch')).toContainText('main');
  // The model line appends the response time as `<model> in <responseTime>`.
  const model = latestMeta.getByTestId('meta-model');
  await expect(model).toBeVisible();
  await expect(model).toContainText('in');
  await expect(latestMeta.getByTestId('meta-response-time')).toBeVisible();

  // Hovering the latest message's timestamp reveals the popover with the three
  // facts (model, cwd, branch) — the response time is on the model line, not in
  // the popover.
  await latestMeta.getByTestId('meta-time').hover();
  const popover = latestMeta.getByTestId('message-meta-popover');
  await expect(popover).toBeVisible();
  await expect(popover).toContainText('model');
  await expect(popover).toContainText('cwd');
  await expect(popover).toContainText('branch');
  await expect(popover).not.toContainText('response time');
});
