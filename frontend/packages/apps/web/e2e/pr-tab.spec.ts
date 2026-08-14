import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The new-session PR tab. The mock backend's `GET /api/prs` handler
 * serves two lenses (reviewer + author) and pairs the reviewer fixture
 * with a no-local-clone entry so both the happy path and the
 * silently-blocked path are exercised here.
 */

test('picking a PR with a local clone pre-fills the composer and a Send carries the worktree request', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page.getByRole('button', { name: 'New session', exact: true }).click();
  // Switch to the PR tab.
  await page.getByTestId('new-session-tab-pr').click();
  // The reviewer section renders the seeded list.
  await expect(page.getByTestId('pr-tab-reviewer')).toBeVisible();

  // Click the PR whose repo has a registered local clone
  // (`x7c1/delta`, see the api-mocks reviewer fixture).
  await page
    .locator('[data-testid="pr-tab-row"][data-has-local-clone="true"]')
    .first()
    .click();

  // The composer chip now shows the registered clone path.
  await expect(page.getByTestId('workdir-chip')).toContainText('delta');

  // The pick decided the branch, so the worktree section is locked to it: a
  // one-line summary, and none of the generic selector's controls.
  await expect(page.getByTestId('worktree-pr-lock')).toContainText(
    'On feat/repo-tab — PR #174’s head branch.',
  );
  await expect(page.getByTestId('worktree-toggle')).toHaveCount(0);
  await expect(page.getByTestId('worktree-start-point')).toHaveCount(0);

  const sendRequest = page.waitForRequest(
    (request) =>
      request.url().endsWith('/api/sends') && request.method() === 'POST',
  );
  // Address the composer textarea explicitly by its placeholder: the
  // new-session screen holds other inputs, so a bare `getByRole('textbox')`
  // is not guaranteed to be unambiguous.
  await page
    .getByPlaceholder('Message to start a new session…')
    .fill('resume PR work');
  await page.getByRole('button', { name: 'Send' }).click();

  const request = await sendRequest;
  expect(request.postDataJSON()).toMatchObject({
    new_session: true,
    text: 'resume PR work',
    workdir: '/home/dev/projects/delta',
    worktree: {
      // The PR head ref is a non-default branch, so the picker cuts
      // the worktree to check the branch out itself (the
      // `use_remote_branch` mode) rather than branching off it.
      start_point: { kind: 'use_remote_branch', name: 'feat/repo-tab' },
    },
  });

  await expect(page.getByTestId('pending-item')).toHaveCount(1);
});

test('picking a PR with Codex selected sends provider "codex" alongside the worktree request', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page.getByRole('button', { name: 'New session', exact: true }).click();
  await page.getByTestId('new-session-tab-pr').click();
  await expect(page.getByTestId('pr-tab-reviewer')).toBeVisible();

  // Choose Codex as the session provider. The selector is the top-level axis
  // of the new-session card and is shared across every tab, so it applies to a
  // PR-origin start too. A PR head ref is checked out via a worktree, which the
  // Codex launch path now honours (previously it rejected any worktree).
  await page.getByTestId('provider-option-codex').click();

  // Click the PR whose repo has a registered local clone (`x7c1/delta`).
  await page
    .locator('[data-testid="pr-tab-row"][data-has-local-clone="true"]')
    .first()
    .click();
  await expect(page.getByTestId('workdir-chip')).toContainText('delta');

  const sendRequest = page.waitForRequest(
    (request) =>
      request.url().endsWith('/api/sends') && request.method() === 'POST',
  );
  await page
    .getByPlaceholder('Message to start a new session…')
    .fill('resume PR work on codex');
  await page.getByRole('button', { name: 'Send' }).click();

  const request = await sendRequest;
  expect(request.postDataJSON()).toMatchObject({
    new_session: true,
    text: 'resume PR work on codex',
    workdir: '/home/dev/projects/delta',
    worktree: {
      start_point: { kind: 'use_remote_branch', name: 'feat/repo-tab' },
    },
    // The chosen provider rides the same send body as the worktree request —
    // this is the whole point of the fix.
    provider: 'codex',
  });

  await expect(page.getByTestId('pending-item')).toHaveCount(1);
});

test('a PR whose repo has no local clone is silently un-clickable with an inline hint', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page.getByRole('button', { name: 'New session', exact: true }).click();
  await page.getByTestId('new-session-tab-pr').click();

  const noCloneRow = page
    .locator('[data-testid="pr-tab-row"][data-has-local-clone="false"]')
    .first();
  await expect(noCloneRow).toBeVisible();
  await expect(noCloneRow).toHaveAttribute('aria-disabled', 'true');
  await expect(
    noCloneRow.getByTestId('pr-tab-row-no-clone-hint'),
  ).toContainText('gh repo clone');

  // Capture the composer's chip text BEFORE the forced click. The
  // Repository tab's mount-time auto-pick has already written its
  // default clone (the first registered repo — `delta` in this
  // fixture) into the composer store, so the chip is visible from
  // the start.
  const chipBefore = await page.getByTestId('workdir-chip').textContent();
  expect(chipBefore).not.toBeNull();

  // Clicking does NOT pre-fill the composer. Playwright's normal
  // `.click()` honours `aria-disabled` and would wait it out, but
  // here we deliberately attempt the click to verify the handler is
  // a no-op: a forced click still routes through React's onClick.
  await noCloneRow.click({ force: true });

  // Asserting the chip didn't change is the direct contract —
  // `not.toBeVisible()` was an indirect proxy that became wrong once
  // mount auto-picks a default. Use `toHaveText`'s built-in polling
  // so we let any (incorrect) state change settle before checking.
  await expect(page.getByTestId('workdir-chip')).toHaveText(chipBefore!);
  // The no-clone fixture is `x7c1/other`; its repo name must not
  // appear in the chip (the click was a no-op).
  await expect(page.getByTestId('workdir-chip')).not.toContainText('other');
});

// The gh-unavailable inline hint behaviour is exercised end-to-end in
// the PRTab component test (`PRTab.test.tsx`) via MSW's `server.use`
// override. Mocking the same behaviour through Playwright would need
// a window-level seam to swap MSW handlers at runtime (the dev mock
// mode's service worker intercepts requests before Playwright's
// network-level `page.route` can see them); leaving that out keeps
// the e2e surface in line with the existing specs, which all rely on
// the seeded MSW fixtures rather than per-test overrides.
