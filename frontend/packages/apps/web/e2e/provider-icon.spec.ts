import { test, expect, type Locator, type Page } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The per-session provider marker in the navigator.
 *
 * Every session card must stay identifiable as Claude Code or Codex at a
 * glance, whatever mix of providers a user runs. The marker is a small
 * monochrome brand mark on the card's meta line (line 2), sharing that line
 * with the last-activity time — deliberately quiet, so it does not compete with
 * the branch name on line 1. The mock fixtures seed four detailed sessions —
 * three on Claude, one on Codex (`feat/codex-adapter`) — which is what makes
 * both marks assertable with no backend; 40 filler sessions sort below them, so
 * reaching the Codex card means walking the paginated list.
 *
 * Unlike the jsdom component tests, the real stylesheet is loaded here, so this
 * is the layer that can prove the mark actually paints in the inherited text
 * color rather than a provider accent hue.
 */

/** The session card whose launch-time branch is `branch`. */
function rowByBranch(page: Page, branch: string): Locator {
  return page.getByTestId('session-node').filter({ hasText: branch });
}

/**
 * Scroll the windowed session list until `row` is mounted. Rows past page 1 are
 * only fetched (and only mounted) once the scroll window reaches them.
 *
 * One screen per attempt — never a jump to `scrollHeight`. The list is both
 * cursor-paginated (mock page size 2) and DOM-windowed, and the Codex seed sits
 * on page 2 with 40 filler sessions below it: jumping to the bottom on every
 * retry keeps pulling in further pages and marches the window PAST the target,
 * unmounting a row that was already reachable, so a retry that lost the first
 * race could never win a later one. Stepping one viewport at a time, and not
 * stepping at all once the row has mounted, cannot overshoot it.
 */
async function scrollUntilVisible(page: Page, row: Locator): Promise<void> {
  const scrollBody = page.getByTestId('sessions-list').locator('..');
  await expect(async () => {
    if ((await row.count()) === 0) {
      await scrollBody.evaluate((el) => {
        el.scrollTop += el.clientHeight;
      });
    }
    await expect(row).toBeVisible({ timeout: 500 });
  }).toPass();
}

test('each session card carries its provider mark on the meta line', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // A Claude session (page 1, not the auto-focused one).
  const claudeRow = rowByBranch(page, 'feat/scratch-ideas');
  await expect(claudeRow).toBeVisible();
  const claudeIcon = claudeRow.getByTestId('session-provider-icon');
  await expect(claudeIcon).toBeVisible();
  await expect(claudeIcon.getByRole('img')).toHaveAttribute(
    'aria-label',
    'Claude Code',
  );
  // The last-activity time appears only on the meta line, so finding it as the
  // mark's sibling places the mark there too — not up on line 1 with the
  // branch name.
  await expect(
    claudeIcon.locator('xpath=..').getByTestId('session-last-activity'),
  ).toBeVisible();

  // The Codex session sorts onto page 2, so walk the windowed list to it.
  const codexRow = rowByBranch(page, 'feat/codex-adapter');
  await scrollUntilVisible(page, codexRow);
  const codexIcon = codexRow.getByTestId('session-provider-icon');
  await expect(codexIcon).toBeVisible();
  await expect(codexIcon.getByRole('img')).toHaveAttribute(
    'aria-label',
    'Codex',
  );
});

test('the provider mark paints in the meta line color, not a provider accent', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  const icon = rowByBranch(page, 'feat/scratch-ideas').getByTestId(
    'session-provider-icon',
  );
  await expect(icon).toBeVisible();

  // The mark is a CSS mask over `background-color: currentColor`, so its
  // painted color must equal the meta line's own text color — the proof that it
  // inherits the subtle foreground tone instead of carrying a provider hue.
  const { markColor, lineColor } = await icon.evaluate((el) => {
    const glyph = el.querySelector('[aria-hidden]');
    if (!(glyph instanceof HTMLElement)) {
      throw new Error('the provider mark rendered no glyph');
    }
    return {
      markColor: getComputedStyle(glyph).backgroundColor,
      lineColor: getComputedStyle(el).color,
    };
  });
  expect(markColor).toBe(lineColor);

  // And the mask is really applied — an unset mask would leave a solid square.
  const mask = await icon.evaluate((el) => {
    const glyph = el.querySelector('[aria-hidden]');
    return glyph instanceof HTMLElement
      ? getComputedStyle(glyph).maskImage ||
          getComputedStyle(glyph).webkitMaskImage
      : '';
  });
  expect(mask).toMatch(/^url\(/);
});
