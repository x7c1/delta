import { test, expect, type Page } from '@playwright/test';

/**
 * End-to-end verification that the Settings appearance picker drives the
 * whole theme pipeline live — `<html data-theme="…">` flips, the downstream
 * CSS-variable token (`--delta-color-surface`) resolves to the new theme
 * block on the same tick, and the registry-driven picker discovers the
 * non-built-in `sepia` theme without any picker-code change.
 *
 * The spec hits each preference the picker exposes (Dark / Light /
 * Sepia / System), so it is the smoke proof both for the already-shipped
 * Dark/Light pair and for the recipe ("add a CSS block + registry entry,
 * the picker handles the rest").
 *
 * No backend scripting is needed: Settings is reachable from the cold-start
 * placeholder, same as `settings-categories.spec.ts`, so no fake scenario
 * is loaded.
 *
 * xterm-bg assertion strategy: the xterm live-update bridge (see the
 * `useEffect(..., [resolvedTheme])` block in `TerminalPane.tsx`) pushes the
 * new background straight into xterm's renderer as a JavaScript option —
 * there is no DOM signal to observe externally. Asserting the canvas pixel
 * is brittle, so we assert the CSS variable downstream of the bridge
 * instead: `terminalBackground()` reads `--delta-color-terminal-bg` off the
 * document root, the same variable this spec verifies updates on a theme
 * flip. (As of writing the terminal background is theme-fixed, so the value
 * does not actually move between dark/light, but the same variable lookup
 * is the contract the bridge is built on; a future theme that flips the
 * terminal background would automatically flow through this assertion.)
 */

const SURFACE_VAR = '--delta-color-surface';

const LIGHT_SURFACE_RGB = '255 255 255';
const DARK_SURFACE_RGB = '29 29 32';
const SEPIA_SURFACE_RGB = '243 233 211';

/** Read a CSS variable resolved on the document root inside the page. */
async function readRootVar(page: Page, name: string): Promise<string> {
  return page.evaluate(
    (varName) =>
      getComputedStyle(document.documentElement).getPropertyValue(varName).trim(),
    name,
  );
}

/** Read the live `<html data-theme="…">` attribute. */
async function readDataTheme(page: Page): Promise<string | null> {
  return page.evaluate(() => document.documentElement.getAttribute('data-theme'));
}

/** Pick the appearance option labelled `value` (the radio is found by role). */
async function pickAppearance(page: Page, value: string): Promise<void> {
  const radio = page.getByTestId(`appearance-option-${value}`).getByRole('radio');
  await radio.check();
  await expect(radio).toBeChecked();
}

test('Settings appearance picker flips data-theme and CSS variables live', async ({
  page,
}) => {
  // Force a known starting state: the inline FOUC script in index.html only
  // honors "dark" / "light" in localStorage; anything else falls back to
  // `prefers-color-scheme`. emulateMedia gives us a deterministic OS signal
  // for the System leg later; the explicit light cycle keeps the cold-start
  // attribute equal to the index.html default.
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');

  await expect(
    page
      .getByTestId('session-node')
      .first()
      .or(page.getByTestId('new-session-empty')),
  ).toBeVisible();

  // Open Settings → Appearance. The picker is reachable from the cold-start
  // navigator footer; no session is needed.
  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByTestId('settings-category-appearance').click();
  await expect(
    dialog.getByTestId('settings-category-appearance'),
  ).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByTestId('appearance-section')).toBeVisible();

  // Cold-start baseline: the index.html default is light, and a fresh
  // install carries no localStorage preference, so `--delta-color-surface`
  // resolves to the light block's value.
  expect(await readDataTheme(page)).toBe('light');
  expect(await readRootVar(page, SURFACE_VAR)).toBe(LIGHT_SURFACE_RGB);

  // Dark: data-theme flips and the surface token follows the dark block.
  await pickAppearance(page, 'dark');
  await expect.poll(() => readDataTheme(page)).toBe('dark');
  await expect.poll(() => readRootVar(page, SURFACE_VAR)).toBe(DARK_SURFACE_RGB);

  // Sepia: proves that adding a `:root[data-theme="<id>"]` block plus a
  // registry entry is enough — the picker enumerates the registry, so this
  // option exists without any picker-code change, and the CSS variable
  // resolves to the new block.
  await pickAppearance(page, 'sepia');
  await expect.poll(() => readDataTheme(page)).toBe('sepia');
  await expect.poll(() => readRootVar(page, SURFACE_VAR)).toBe(SEPIA_SURFACE_RGB);

  // Light: round-trip back so the reverse direction is asserted too.
  await pickAppearance(page, 'light');
  await expect.poll(() => readDataTheme(page)).toBe('light');
  await expect.poll(() => readRootVar(page, SURFACE_VAR)).toBe(LIGHT_SURFACE_RGB);

  // System: with `prefers-color-scheme: dark` the picker resolves to the
  // dark block live (no reload). emulateMedia drives matchMedia in
  // Chromium so the `'system'` reactive subscription in useTheme picks
  // up the change.
  await page.emulateMedia({ colorScheme: 'dark' });
  await pickAppearance(page, 'system');
  await expect.poll(() => readDataTheme(page)).toBe('dark');
  await expect.poll(() => readRootVar(page, SURFACE_VAR)).toBe(DARK_SURFACE_RGB);

  // And flip the OS back to light → the resolution follows without a
  // separate user pick, which is the whole point of the System option.
  await page.emulateMedia({ colorScheme: 'light' });
  await expect.poll(() => readDataTheme(page)).toBe('light');
  await expect.poll(() => readRootVar(page, SURFACE_VAR)).toBe(LIGHT_SURFACE_RGB);

  await dialog.getByTestId('settings-close').click();
  await expect(dialog).toHaveCount(0);
});
