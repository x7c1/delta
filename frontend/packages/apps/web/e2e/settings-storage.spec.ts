import { test, expect } from '@playwright/test';
import { MOCK_DATA_DIR } from '@delta/api-mocks';
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
