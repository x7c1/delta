import { test, expect } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The opt-in git-worktree option on the new-session directory picker.
 *
 * When the selected directory is a git repository (the mock repo
 * `/home/dev/projects/delta`), a toggle offers to start the session in a fresh
 * worktree, with a start-point choice. This drives the full UI flow — select the
 * git-repo recent directory, toggle the worktree on, pick a start-point, send —
 * and asserts the `POST /api/sends` body carried the `worktree` request.
 *
 * It also covers the negative case (a non-git directory shows no toggle) so the
 * default non-worktree flow stays intact.
 */

/** Select a recent directory in the new-session picker by its full path. */
async function startNewSessionIn(
  page: import('@playwright/test').Page,
  path: string,
): Promise<void> {
  await page.getByRole('button', { name: 'New session', exact: true }).click();
  // The Recent rows are looked up by their stable `title` (full path); the
  // visible label is abbreviated. Scope to the Recent section so the lookup is
  // not ambiguous with a Browse entry that shares the same absolute path.
  await page.getByTestId('workdir-recent').getByTitle(path).click();
  await page.getByTestId('workdir-confirm').click();
  await expect(page.getByTestId('new-session-empty')).toBeVisible();
}

test('opting into a worktree carries the start-point on the send', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await startNewSessionIn(page, '/home/dev/projects/delta');

  // The directory is a git repo, so the worktree toggle appears.
  const toggle = page.getByTestId('worktree-toggle');
  await expect(toggle).toBeVisible();
  await toggle.check();

  // The start-point selector defaults to Current HEAD; switch to the default
  // branch preset ("Latest main").
  await expect(page.getByTestId('start-point-head')).toBeChecked();
  await page.getByTestId('start-point-default-branch').check();

  // Capture the outgoing send so we can assert it carried the worktree request.
  const sendRequest = page.waitForRequest(
    (request) =>
      request.url().endsWith('/api/sends') && request.method() === 'POST',
  );

  await page.getByRole('textbox').fill('start in a worktree');
  await page.getByRole('button', { name: 'Send' }).click();

  const request = await sendRequest;
  expect(request.postDataJSON()).toMatchObject({
    new_session: true,
    text: 'start in a worktree',
    workdir: '/home/dev/projects/delta',
    worktree: { start_point: { kind: 'remote_branch', name: 'main' } },
  });

  // The send is accepted: the optimistic pending chip shows.
  await expect(page.getByTestId('pending-item')).toHaveCount(1);
});

test('choosing "use this branch" carries use_remote_branch on the send', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await startNewSessionIn(page, '/home/dev/projects/delta');

  const toggle = page.getByTestId('worktree-toggle');
  await expect(toggle).toBeVisible();
  await toggle.check();

  // Pick the default-branch preset ("Latest main"); the use-vs-new choice
  // appears, defaulting to "New branch from it".
  await page.getByTestId('start-point-default-branch').check();
  await expect(page.getByTestId('branch-mode-new')).toBeChecked();

  // Switch to "Use this branch".
  await page.getByTestId('branch-mode-use').check();

  const sendRequest = page.waitForRequest(
    (request) =>
      request.url().endsWith('/api/sends') && request.method() === 'POST',
  );

  await page.getByRole('textbox').fill('work on main directly');
  await page.getByRole('button', { name: 'Send' }).click();

  const request = await sendRequest;
  expect(request.postDataJSON()).toMatchObject({
    new_session: true,
    text: 'work on main directly',
    workdir: '/home/dev/projects/delta',
    worktree: { start_point: { kind: 'use_remote_branch', name: 'main' } },
  });

  await expect(page.getByTestId('pending-item')).toHaveCount(1);
});

test('the worktree toggle is absent for a non-git directory', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // `/home/dev/scratch` is outside the mock git repo, so no toggle is offered.
  await startNewSessionIn(page, '/home/dev/scratch');
  await expect(page.getByTestId('worktree-toggle')).toHaveCount(0);

  // The plain (non-worktree) flow is unaffected: the send omits `worktree`.
  const sendRequest = page.waitForRequest(
    (request) =>
      request.url().endsWith('/api/sends') && request.method() === 'POST',
  );
  await page.getByRole('textbox').fill('plain non-git session');
  await page.getByRole('button', { name: 'Send' }).click();

  const request = await sendRequest;
  const body = request.postDataJSON();
  expect(body).toMatchObject({
    new_session: true,
    text: 'plain non-git session',
    workdir: '/home/dev/scratch',
  });
  expect(body.worktree).toBeUndefined();
});
