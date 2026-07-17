import type { Locator } from '@playwright/test';
import { test, expect, type Page } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * End-to-end verification that the Settings visual-effects control drives the
 * decorative-rendering gate live, against the real CSS: picking Off/On flips
 * `<html data-effects="…">` and the two gated card call sites (the navigator
 * session card and the transcript composer card) drop / restore their
 * drop-shadow on the same tick — no reload — while a functional overlay
 * shadow (the Settings dialog panel, `shadow-xl`) is untouched.
 *
 * `on`/`off` resolve independently of the platform, so the assertions hold in
 * the Chromium project this suite runs under (where `auto` would resolve to
 * the rich look). A session is spawned first (scenario `first-send`) so both
 * gated cards are mounted before the setting is toggled.
 */

// The card drop-shadow's color (Tailwind `shadow-md`, `rgb(0 0 0 / 0.1)`),
// normalized to the form `getComputedStyle` reports in Chromium. Its presence
// in a computed `box-shadow` marks the rich decorative shadow; its absence
// (the gate replaces it with a transparent layer) marks the flat look.
const CARD_SHADOW_COLOR = 'rgba(0, 0, 0, 0.1)';

/** Read an element's resolved `box-shadow`. */
async function boxShadow(locator: Locator): Promise<string> {
  return locator.evaluate((el) => getComputedStyle(el).boxShadow);
}

/** Read the live `<html data-effects="…">` attribute. */
async function readDataEffects(page: Page): Promise<string | null> {
  return page.evaluate(() =>
    document.documentElement.getAttribute('data-effects'),
  );
}

/** Pick the visual-effects option `value` inside the open Settings dialog. */
async function pickEffects(dialog: Locator, value: string): Promise<void> {
  const radio = dialog
    .getByTestId(`appearance-effects-option-${value}`)
    .getByRole('radio');
  await radio.check();
  await expect(radio).toBeChecked();
}

test('Settings visual-effects control gates card shadows live without a reload', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'first-send hold then answer');

  // Both gated cards are mounted once the session is focused: the navigator
  // session card and the transcript composer card.
  const sessionCard = page.getByTestId('session-card').first();
  const composerCard = page.getByTestId('composer-card');
  await expect(sessionCard).toBeVisible();
  await expect(composerCard).toBeVisible();

  // Baseline: no explicit setting yet. The Chromium project resolves `auto`
  // to the rich look, so both cards carry the drop-shadow.
  expect(await boxShadow(composerCard)).toContain(CARD_SHADOW_COLOR);
  expect(await boxShadow(sessionCard)).toContain(CARD_SHADOW_COLOR);

  // Open Settings → Appearance.
  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByTestId('settings-category-appearance').click();
  await expect(dialog.getByTestId('appearance-section')).toBeVisible();

  // Off → flat: the attribute flips and both cards drop their shadow live.
  await pickEffects(dialog, 'off');
  await expect.poll(() => readDataEffects(page)).toBe('flat');
  await expect.poll(() => boxShadow(composerCard)).not.toContain(CARD_SHADOW_COLOR);
  await expect.poll(() => boxShadow(sessionCard)).not.toContain(CARD_SHADOW_COLOR);

  // The Settings dialog panel is a functional overlay (`shadow-xl`) and must
  // NOT be gated: its shadow survives under the flat look.
  expect(await boxShadow(dialog)).toContain(CARD_SHADOW_COLOR);

  // On → rich: the shadows return, again with no reload.
  await pickEffects(dialog, 'on');
  await expect.poll(() => readDataEffects(page)).toBe('rich');
  await expect.poll(() => boxShadow(composerCard)).toContain(CARD_SHADOW_COLOR);
  await expect.poll(() => boxShadow(sessionCard)).toContain(CARD_SHADOW_COLOR);

  await dialog.getByTestId('settings-close').click();
  await expect(dialog).toHaveCount(0);
});
