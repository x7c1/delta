import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * Permission decisions made in the browser — through the real loop.
 *
 * Scenario `permission-decide`: the fake calls a tool, fires
 * `PermissionRequest` and BLOCKS on the hook response, exactly like the real
 * `claude` awaiting its permission hook. The notice (tool name + input
 * summary + Allow/Deny) appears in the browser; clicking a button POSTs
 * `/api/permissions/{id}/decision`, which wakes the blocked hook with
 * `hookSpecificOutput.decision.behavior` — and the fake continues down the
 * matching `on_allow`/`on_deny` branch, so the visible conversation proves
 * which decision Claude actually received.
 *
 * The browser must answer before the server's decision deadline
 * (`DELTA_PERMISSION_DECISION_TIMEOUT_MS` in the harness); the clicks below
 * happen as soon as the notice renders, well inside it.
 */

test('allowing a permission in the browser lets the tool proceed', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'permission-decide then ask before the tool');

  // The dialog notice appears with the tool's name and what it wants to do.
  const notice = page.getByTestId('permission-notice');
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('Permission requested: Bash');
  await expect(notice).toContainText('rm -rf scratch');

  // Allow in the browser: the decision clears the notice (the broadcast
  // `permission_resolved`) and unblocks the fake's hook wait.
  await notice.getByRole('button', { name: 'Allow' }).click();
  await expect(notice).toHaveCount(0);

  // The fake received `behavior: "allow"` and ran its on_allow branch: the
  // tool result lands and the turn completes.
  await expect(page.getByText('tool ran after allow')).toBeVisible();
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});

test('denying a permission in the browser stops the tool', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'permission-decide then deny the tool');

  const notice = page.getByTestId('permission-notice');
  await expect(notice).toBeVisible();

  await notice.getByRole('button', { name: 'Deny' }).click();
  await expect(notice).toHaveCount(0);

  // The fake received `behavior: "deny"` and ran its on_deny branch.
  await expect(page.getByText('tool was denied')).toBeVisible();
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});
