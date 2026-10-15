import { test, expect } from './support/fixtures';
import { waitForSettledApp } from './support/app';
import { AUTH_TOKEN_STORAGE_KEY } from './support/endpoints';

/**
 * The page's one-time code handoff, end to end through the Vite dev server's
 * proxy to the real backend: a page holding no token shows the screen that
 * asks for a link; a URL with a freshly minted code opens the app (and the
 * code leaves the address bar); the same URL a second time still opens the
 * app while a token is stored, and is refused once none is.
 *
 * Every other spec starts holding the fixed token (see the config's
 * `storageState`); this one starts with none.
 */
test.use({ storageState: { cookies: [], origins: [] } });

test('a one-time code opens the app once', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('reopen-screen')).toBeVisible();

  // Minted the way `delta-server open-url` mints one: an authenticated POST
  // (the config sends the bearer token on direct requests).
  const minted = await page.request.post('/api/auth/codes');
  expect(minted.status()).toBe(200);
  const { code } = (await minted.json()) as { code: string };

  // From another document, so the page loads afresh rather than only
  // changing its fragment.
  await page.goto('about:blank');
  await page.goto(`/#code=${code}`);
  await waitForSettledApp(page);
  expect(new URL(page.url()).hash).toBe('');
  expect(
    await page.evaluate(
      (key) => window.localStorage.getItem(key),
      AUTH_TOKEN_STORAGE_KEY,
    ),
  ).not.toBeNull();

  // A reload keeps working on the stored token, with no code.
  await page.reload();
  await waitForSettledApp(page);

  // The same, already-used link while the token is stored still opens the app
  // on that token, and the code leaves the address bar.
  await page.goto('about:blank');
  await page.goto(`/#code=${code}`);
  await waitForSettledApp(page);
  expect(new URL(page.url()).hash).toBe('');
  expect(
    await page.evaluate(
      (key) => window.localStorage.getItem(key),
      AUTH_TOKEN_STORAGE_KEY,
    ),
  ).not.toBeNull();

  // With no stored token, the same used link is refused.
  await page.evaluate(
    (key) => window.localStorage.removeItem(key),
    AUTH_TOKEN_STORAGE_KEY,
  );
  await page.goto('about:blank');
  await page.goto(`/#code=${code}`);
  await expect(page.getByTestId('reopen-screen')).toBeVisible();
  await expect(page.getByRole('status')).toContainText('already been used');
});
