import { test, expect } from './support/fixtures';

/**
 * The copy-and-adapt flow for a launch option Delta ships, end to end against
 * the real backend.
 *
 * The Codex `config` built-in is a starting point: a real `config` carries
 * machine-specific paths (`sandbox_workspace_write.writable_roots`) the shipped
 * row cannot know, so the user reads it, duplicates it, adds their paths and
 * registers a row of their own. This spec walks exactly that.
 *
 * Copying is not forced by exclusivity — `config` is the one `thread/start`
 * field a launch may select twice, and the adapter deep-merges every selected
 * `config` into one object — but it is still how a row with your own paths comes
 * to exist, and duplicating the shipped one means not having to discover the
 * JSON key names first.
 *
 * What it pins, in order:
 *
 * 1. The shipped row is **there** with no seeding at all — the boot-time
 *    reconcile in the composition root materialized it into a per-run database
 *    — badged as Delta's own.
 * 2. Its value is **shown in full**, with no interaction at all — reading the
 *    JSON object is the point of the row, so a registered `config` must be
 *    legible the moment the list renders.
 * 3. `Duplicate` fills the add form from it.
 * 4. An edited copy registers as an ordinary, non-built-in row — which is what
 *    a `Delete` control on it proves.
 * 5. The shipped row itself **cannot** be deleted: no control in the UI, and a
 *    `409` from the API for anyone who asks anyway.
 */

/** The label of the Codex `config` option Delta ships. */
const SHIPPED_LABEL = 'Config: reasoning summary';

/** The label the spec registers its own copy under. */
const COPY_LABEL = 'My config';

/** Ids this spec registered, handed back so the next spec sees a clean registry. */
const registered: number[] = [];

test.afterEach(async ({ page }) => {
  while (registered.length > 0) {
    await page.request.delete(`/api/launch-options/${registered.pop()}`);
  }
});

test('a shipped Codex config option is readable, duplicable and undeletable', async ({
  page,
}) => {
  await page.goto('/');
  await expect(
    page.getByTestId('session-node').first().or(page.getByTestId('new-session-empty')),
  ).toBeVisible();

  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByTestId('launch-options-section')).toBeVisible();

  // The registry is provider-scoped; the shipped `config` option is Codex's.
  await dialog.getByTestId('launch-option-provider-codex').click();
  const list = dialog.getByTestId('launch-options-list');
  const shipped = list
    .locator('[data-testid^="launch-option-row-"]')
    .filter({ hasText: SHIPPED_LABEL });

  // 1. It is there, with nothing seeded, and it is badged as Delta's.
  await expect(shipped).toHaveCount(1);
  await expect(shipped.getByText('Built-in')).toBeVisible();

  const rowTestId = await shipped.getAttribute('data-testid');
  const shippedId = Number(rowTestId?.replace('launch-option-row-', ''));
  expect(Number.isFinite(shippedId)).toBe(true);

  // The declared catalog is the source of truth for the value, so read it from
  // the API rather than restating it here: a catalog edit must not have to be
  // mirrored into this spec to keep it honest.
  const listed = (await (
    await page.request.get('/api/launch-options')
  ).json()) as {
    launch_options: { id: number; value: string | null; builtin: boolean }[];
  };
  const shippedValue = listed.launch_options.find(
    (option) => option.id === shippedId,
  )?.value;
  expect(shippedValue).toBeTruthy();

  // 2. The row shows that value in full, with nothing clicked — not a
  // truncation of it.
  await expect(
    shipped.getByTestId(`launch-option-value-${shippedId}`),
  ).toHaveText(shippedValue!);

  // 3. Duplicate seeds the add form from the row.
  await shipped
    .getByRole('button', { name: /^Duplicate launch option/ })
    .click();
  const labelInput = dialog.getByLabel('Label (optional)');
  const valueInput = dialog.getByLabel('Value (optional)');
  await expect(dialog.getByLabel('Name (the field)')).toHaveValue('config');
  await expect(labelInput).toHaveValue(SHIPPED_LABEL);
  await expect(valueInput).toHaveValue(shippedValue!);

  // 4. The copy is edited and registered — an ordinary row of the user's own.
  const edited = JSON.stringify({
    model_reasoning_summary: 'auto',
    sandbox_workspace_write: { writable_roots: ['/tmp/delta-e2e-fake'] },
  });
  await labelInput.fill(COPY_LABEL);
  await valueInput.fill(edited);
  await dialog.getByRole('button', { name: 'Add option' }).click();

  const copy = list
    .locator('[data-testid^="launch-option-row-"]')
    .filter({ hasText: COPY_LABEL });
  await expect(copy).toHaveCount(1);
  await expect(copy.getByText('Built-in')).toHaveCount(0);
  await expect(
    copy.getByRole('button', { name: /^Delete launch option/ }),
  ).toBeVisible();

  const copyTestId = await copy.getAttribute('data-testid');
  registered.push(Number(copyTestId?.replace('launch-option-row-', '')));

  // And the copy really carries the edited value, not the shipped one.
  await expect(
    copy.locator('[data-testid^="launch-option-value-"]'),
  ).toHaveText(edited);

  // 5. The shipped row offers no delete control at all…
  await expect(
    shipped.getByRole('button', { name: /^Delete launch option/ }),
  ).toHaveCount(0);

  // …and the API refuses it with a 409, leaving the row registered.
  const refused = await page.request.delete(
    `/api/launch-options/${shippedId}`,
  );
  expect(refused.status()).toBe(409);
  await expect(shipped).toHaveCount(1);

  await dialog.getByTestId('settings-close').click();
});
