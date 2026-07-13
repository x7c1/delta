import { test, expect, type Locator } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * A pending branch selection (the "Branch from selected text" banner) is
 * dismissed by a plain click anywhere in the transcript body — whether the
 * click lands on empty space or directly on the still-highlighted text.
 *
 * The dismissal must not depend on when the browser collapses the native
 * selection relative to the click: a click on the selected text itself can
 * leave the selection alive through the click event (deferred collapse), so the
 * app detects "was this the release of a drag-select?" from the pointer's own
 * travel, not from selection state. This spec drives the real mouse to cover
 * both the empty-gap click and the regression case of clicking on the selected
 * text.
 */

/**
 * Geometry of one transcript message: the first rendered text line's box (tight
 * around the glyphs, so a drag along it actually crosses characters) and a point
 * in the article's left padding column, which is a reliable textless spot still
 * over the transcript body (the click listener's element).
 */
async function messageGeometry(message: Locator): Promise<{
  lineLeft: number;
  lineRight: number;
  lineMidY: number;
  padLeftX: number;
}> {
  return await message.evaluate((article) => {
    const content = article.querySelector('[class*="space-y"]') ?? article;
    const range = document.createRange();
    range.selectNodeContents(content);
    // The first client rect is the first visual line, tight around its glyphs;
    // dragging along its vertical centre crosses real characters (dragging
    // across the whole article box tends to miss glyphs entirely).
    const line = range.getClientRects()[0];
    const articleRect = article.getBoundingClientRect();
    return {
      lineLeft: line.left,
      lineRight: line.right,
      lineMidY: line.top + line.height / 2,
      // A few px inside the article's left edge — inside the message's own
      // padding column, so it is guaranteed textless yet still over the body.
      padLeftX: articleRect.left + 4,
    };
  });
}

/** Drag-select across a message's first line to arm a pending branch. */
async function dragSelect(message: Locator): Promise<void> {
  const geo = await messageGeometry(message);
  await message.page().mouse.move(geo.lineLeft + 2, geo.lineMidY);
  await message.page().mouse.down();
  await message.page().mouse.move(geo.lineRight - 2, geo.lineMidY, { steps: 12 });
  await message.page().mouse.up();
}

test('a plain click in the transcript dismisses a pending branch selection', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  const banner = page.getByText('Branch from selected text');
  // Bring the conversation to the bottom so the target message sits clear of the
  // pinned top-region overlay, which would otherwise intercept the pointer.
  await page.locator('[data-testid="message-item"]').last().scrollIntoViewIfNeeded();

  // A clean single-passage user message mid-viewport (the very first message is
  // pinned near the top edge under the timeline overlay).
  const message = page
    .locator('[data-testid="message-item"]')
    .filter({ hasText: 'List the files here.' });

  // 1. Drag-selecting message text arms the banner (and the release click, which
  //    carries the drag's pointer travel, does NOT dismiss it).
  await dragSelect(message);
  await expect(banner).toBeVisible();

  // 2. A plain click on a textless gap (the message's left padding column)
  //    dismisses it.
  const gap = await messageGeometry(message);
  await page.mouse.click(gap.padLeftX, gap.lineMidY);
  await expect(banner).toHaveCount(0);

  // 3. Re-select, then plain-click DIRECTLY on the selected text — the confirmed
  //    cross-engine failure case (the engine may defer collapsing the selection
  //    past the click). It must dismiss all the same.
  await dragSelect(message);
  await expect(banner).toBeVisible();

  const onText = await messageGeometry(message);
  await page.mouse.click((onText.lineLeft + onText.lineRight) / 2, onText.lineMidY);
  await expect(banner).toHaveCount(0);
});
