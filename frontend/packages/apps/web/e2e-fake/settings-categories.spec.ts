import { test, expect } from '@playwright/test';

/**
 * The Settings dialog hosts a VS Code-style 2-pane layout: a vertical category
 * rail on the left and the active category's content on the right. Clicking a
 * category swaps the right pane, and the previously visible section unmounts
 * so two categories never render simultaneously.
 *
 * The spec opens the dialog from the navigator gear, asserts the default
 * landing pane (Launch options) is the only one rendered, switches to
 * Repository scan roots, asserts the rail's selection follows and the right
 * pane content has swapped, and switches back to confirm the round-trip.
 *
 * No backend scripting is required — the spec touches only the registry UI
 * (the launch-options list and the scan-roots empty state), both of which
 * render purely from REST data the real backend already serves.
 */
test('Settings dialog swaps the right pane when categories are clicked', async ({
  page,
}) => {
  await page.goto('/');

  // The cold-start placeholder OR an existing session node settles the app
  // before reaching for the gear. Either is fine — both render with the
  // navigator's settings entry mounted in the footer.
  await expect(
    page.getByTestId('session-node').first().or(page.getByTestId('new-session-empty')),
  ).toBeVisible();

  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // Default landing pane is Launch options; the scan-roots section is not
  // mounted yet.
  await expect(
    dialog.getByTestId('settings-category-launch-options'),
  ).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByTestId('launch-options-section')).toBeVisible();
  await expect(dialog.getByTestId('scan-roots-section')).toHaveCount(0);

  // Switch to Repository scan roots: the rail's selection follows, the
  // scan-roots section appears, and the launch-options section unmounts.
  await dialog.getByTestId('settings-category-scan-roots').click();
  await expect(
    dialog.getByTestId('settings-category-scan-roots'),
  ).toHaveAttribute('aria-selected', 'true');
  await expect(
    dialog.getByTestId('settings-category-launch-options'),
  ).toHaveAttribute('aria-selected', 'false');
  await expect(dialog.getByTestId('scan-roots-section')).toBeVisible();
  await expect(dialog.getByTestId('launch-options-section')).toHaveCount(0);

  // And back: the launch-options pane returns.
  await dialog.getByTestId('settings-category-launch-options').click();
  await expect(dialog.getByTestId('launch-options-section')).toBeVisible();
  await expect(dialog.getByTestId('scan-roots-section')).toHaveCount(0);

  await dialog.getByTestId('settings-close').click();
  await expect(dialog).toHaveCount(0);
});
