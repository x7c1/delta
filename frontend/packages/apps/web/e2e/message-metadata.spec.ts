import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The per-message metadata line and its info-icon popover render in the
 * conversation view.
 *
 * The seeded main thread's messages carry the transcript-derived metadata
 * (model, response time, cwd, git branch). This asserts the structural
 * behavior: the latest assistant message shows the two-line meta (model and the
 * cwd/branch working location), and the info icon reveals a popover listing the
 * four facts. It never asserts styling or response wording.
 */
test('the latest assistant message shows the two-line meta and an info popover', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The seeded main thread renders its transcript.
  await expect(page.getByTestId('message-item').first()).toBeVisible();

  // The latest assistant message carries the richer two-line meta: the model on
  // line 1 and the cwd/branch working location on line 2.
  const latestMeta = page.locator('[data-testid="message-meta"][data-latest="true"]');
  await expect(latestMeta).toHaveCount(1);
  await expect(latestMeta.getByTestId('meta-model')).toBeVisible();
  await expect(latestMeta.getByTestId('meta-location')).toBeVisible();
  await expect(latestMeta.getByTestId('meta-cwd')).toContainText('/home/dev/repo');
  await expect(latestMeta.getByTestId('meta-branch')).toContainText('main');

  // Hovering the latest message's info icon reveals the popover with the four
  // facts (model, response time, cwd, branch).
  await latestMeta.getByTestId('message-meta-info').hover();
  const popover = latestMeta.getByTestId('message-meta-popover');
  await expect(popover).toBeVisible();
  await expect(popover).toContainText('model');
  await expect(popover).toContainText('response time');
  await expect(popover).toContainText('cwd');
  await expect(popover).toContainText('branch');
});
