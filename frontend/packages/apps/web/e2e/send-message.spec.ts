import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * Sending a message surfaces it in the active thread's pending queue.
 *
 * In mock mode `POST /api/sends` accepts the send and the app shows it
 * optimistically in the transcript's pending queue (FIFO). This asserts the
 * structural behavior — the typed text becomes a queued entry attributed to the
 * active thread — not any styling or response content.
 */
test('a sent message appears in the transcript pending queue', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The seeded main thread renders its transcript.
  await expect(page.getByTestId('message-item').first()).toBeVisible();

  const composer = page.getByRole('textbox');
  await composer.fill('hello from e2e');
  await page.getByRole('button', { name: 'Send' }).click();

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('hello from e2e');
});
