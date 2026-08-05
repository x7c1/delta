import { test, expect, type Locator, type Page } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The per-session provider marker in the navigator.
 *
 * Every session card must stay identifiable as Claude Code or Codex at a
 * glance, whatever mix of providers a user runs. The marker is the card's
 * kebab-menu trigger: its three dots rest in the provider's accent hue instead
 * of the default subtle gray — no extra element in either text line, and the
 * trigger's accessible name carries the provider for readers who cannot rely
 * on the hue. The mock fixtures seed four detailed sessions — three on Claude,
 * one on Codex (`feat/codex-adapter`) — which is what makes both hues
 * assertable with no backend; 40 filler sessions sort below them, so reaching
 * the Codex card means walking the paginated list.
 *
 * Unlike the jsdom component tests, the real stylesheet is loaded here, so
 * this is the layer that can prove the tint actually paints: the two providers
 * resolve to different colors, and both differ from the meta line's resting
 * text tone.
 */

/** The session card whose launch-time branch is `branch`. */
function rowByBranch(page: Page, branch: string): Locator {
  return page.getByTestId('session-node').filter({ hasText: branch });
}

/** The card's kebab trigger — the tinted provider marker. */
function triggerOf(row: Locator): Locator {
  return row
    .locator('..')
    .getByRole('button', { name: /^Session actions for/ });
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

test('each card tints its kebab trigger in its provider hue', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // A Claude session (page 1, not the auto-focused one). The accessible name
  // names the provider — the non-color channel of the marker.
  const claudeRow = rowByBranch(page, 'feat/scratch-ideas');
  await expect(claudeRow).toBeVisible();
  const claudeTrigger = triggerOf(claudeRow);
  await expect(claudeTrigger).toHaveAccessibleName(/\(Claude Code session\)$/);
  const claudeColor = await claudeTrigger.evaluate(
    (el) => getComputedStyle(el).color,
  );

  // The tint replaces the resting subtle tone: the trigger's color must differ
  // from the meta line's own text color on the same card.
  const lineColor = await claudeRow
    .getByTestId('session-last-activity')
    .evaluate((el) => getComputedStyle(el).color);
  expect(claudeColor).not.toBe(lineColor);

  // The Codex session sorts onto page 2, so walk the windowed list to it. Its
  // trigger must resolve to a different hue than the Claude one — the proof
  // the two providers stay tellable apart.
  const codexRow = rowByBranch(page, 'feat/codex-adapter');
  await scrollUntilVisible(page, codexRow);
  const codexTrigger = triggerOf(codexRow);
  await expect(codexTrigger).toHaveAccessibleName(/\(Codex session\)$/);
  const codexColor = await codexTrigger.evaluate(
    (el) => getComputedStyle(el).color,
  );
  expect(codexColor).not.toBe(lineColor);
  expect(codexColor).not.toBe(claudeColor);
});
