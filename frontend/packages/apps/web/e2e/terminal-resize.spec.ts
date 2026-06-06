import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The terminal column is resizable by dragging its divider.
 *
 * On large screens the terminal is a persistent pane with a left-edge resize
 * handle. The terminal sits on the right, so dragging the handle left widens
 * the pane (its width is `window.innerWidth - pointerX`). This drags the handle
 * left and asserts the pane grew, exercising the structural resize behavior.
 */
test('the terminal column width is resizable by dragging the divider', async ({
  page,
}) => {
  await useManualEventControl(page);
  // A wide viewport so the terminal is the persistent, resizable pane (>= lg).
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');

  // Open the terminal pane.
  await page.getByRole('button', { name: 'Terminal' }).click();

  const handle = page.getByRole('separator', { name: 'Resize terminal' });
  await expect(handle).toBeVisible();

  // The resizable pane is the handle's parent (it carries the inline width).
  const pane = page.locator('div', { has: handle }).last();
  const before = await pane.boundingBox();
  expect(before).not.toBeNull();

  // Drag the handle ~120px to the left, which widens the right-anchored pane.
  const start = await handle.boundingBox();
  expect(start).not.toBeNull();
  const startX = start!.x + start!.width / 2;
  const startY = start!.y + start!.height / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX - 120, startY, { steps: 10 });
  await page.mouse.up();

  const after = await pane.boundingBox();
  expect(after).not.toBeNull();
  expect(after!.width).toBeGreaterThan(before!.width + 40);
});
