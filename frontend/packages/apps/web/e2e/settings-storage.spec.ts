import { test, expect, type Page } from '@playwright/test';
import { MOCK_DATA_DIR, mockStorage, mockStorageWorktrees } from '@delta/api-mocks';
import { useManualEventControl } from './support/app';

/**
 * Settings → Storage shows where the running server keeps its files. In mock
 * mode MSW serves a fixed inventory, so the data directory it reports is
 * assertable verbatim once the category is selected.
 */
test('the Storage category shows the data directory the server reports', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  await dialog.getByTestId('settings-category-storage').click();
  const section = dialog.getByTestId('storage-section');
  await expect(section).toBeVisible();
  await expect(
    section.getByTitle(MOCK_DATA_DIR, { exact: true }),
  ).toBeVisible();
  await expect(section.getByTestId('storage-database')).toContainText('3.1 MB');
});

/** Open Settings → Storage and return the section. */
async function openStorage(page: Page) {
  await useManualEventControl(page);
  await page.goto('/');
  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await dialog.getByTestId('settings-category-storage').click();
  const section = dialog.getByTestId('storage-section');
  await expect(section).toBeVisible();
  return section;
}

test('old closed sessions are removed in bulk after a confirmation naming the count', async ({
  page,
}) => {
  const section = await openStorage(page);
  const block = section.getByTestId('storage-prune');
  const preview = block.getByTestId('storage-prune-preview');
  // Every seeded closed session is months old; the open one is never counted.
  await expect(preview).toHaveText(/^\d+ closed sessions older than 30 days\.$/);
  const count = Number((await preview.textContent())?.split(' ')[0]);
  expect(count).toBeGreaterThan(0);

  await block.getByRole('button', { name: 'Remove…' }).click();
  await block
    .getByTestId('storage-prune-confirm')
    .getByRole('button', { name: `Remove ${count} sessions` })
    .click();

  const result = block.getByTestId('storage-prune-result');
  await expect(result).toContainText(`Removed ${count} sessions; skipped 1.`);
  await expect(result.getByTestId('storage-prune-skipped')).toContainText('it is open');
  await expect(preview).toHaveText('0 closed sessions older than 30 days.');
});

test('a leftover worktree with changes is removed only once its name is typed', async ({
  page,
}) => {
  const section = await openStorage(page);
  const dirty = mockStorageWorktrees.find((worktree) => worktree.dirty === true)!;
  const name = dirty.path.slice(dirty.path.lastIndexOf('/') + 1);
  const row = section
    .getByTestId('storage-worktree')
    .filter({ has: page.getByTitle(dirty.path, { exact: true }) });
  await expect(row.getByTestId('storage-worktree-state')).toHaveText('Has uncommitted changes');

  await row.getByRole('button', { name: `Remove worktree ${name}` }).click();
  const remove = row.getByRole('button', { name: 'Remove and lose changes' });
  await expect(remove).toBeDisabled();
  await row.getByLabel('Directory name to confirm').fill(name);
  await remove.click();

  await expect(section.getByTitle(dirty.path, { exact: true })).toHaveCount(0);
  await expect(section.getByTestId('storage-worktree')).toHaveCount(
    mockStorageWorktrees.length - 1,
  );
});

test('a migration snapshot is deleted after a confirmation naming it', async ({ page }) => {
  const section = await openStorage(page);
  const [first, second] = mockStorage.snapshots;
  const snapshots = section.getByTestId('storage-snapshots');

  await snapshots.getByRole('button', { name: 'Delete snapshot delta.db.bak-v3' }).click();
  const confirm = snapshots.getByTestId('storage-snapshot-confirm');
  await expect(confirm).toContainText('Delete delta.db.bak-v3 (1.0 MB)?');
  await confirm.getByRole('button', { name: 'Delete snapshot' }).click();

  await expect(snapshots.getByTitle(first.path, { exact: true })).toHaveCount(0);
  await expect(snapshots.getByTitle(second.path, { exact: true })).toBeVisible();
});
