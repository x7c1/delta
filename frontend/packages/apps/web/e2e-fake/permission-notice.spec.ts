import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * The permission notice appears only for a genuine interactive dialog.
 *
 * `PreToolUse` fires for every tool call (auto-approved ones included), so it
 * must not raise the notice; `PermissionRequest` fires only when the TUI
 * actually shows a dialog, and that is what notifies the browser. With no
 * browser decision before the server's deadline, the hook passes through to
 * the TUI prompt; the notice then resolves on its own when the matching
 * `tool_result` lands in the transcript — the human answered in the TUI, no
 * browser action involved.
 */

test('a tool call without a dialog never raises the permission notice', async ({
  page,
}) => {
  await page.goto('/');
  // Scenario `pre-tool-only`: tool_use + PreToolUse, tool_result, reply, stop —
  // and no PermissionRequest at any point.
  await startNewSession(page, 'pre-tool-only run a quiet tool');

  // The turn completes: the prompt, the tool call (its result absorbed
  // inline), and the closing reply render as three items.
  await expect(page.getByTestId('message-item')).toHaveCount(3);
  await expect(page.getByTestId('pending-item')).toHaveCount(0);

  // At no point did a notice appear; asserting after completion covers the
  // whole turn because the notice only clears via an explicit resolution.
  await expect(page.getByTestId('permission-notice')).toHaveCount(0);
});

test('a real dialog raises the notice and the tool result resolves it', async ({
  page,
}) => {
  await page.goto('/');
  // Scenario `permission-dialog`: tool_use + PreToolUse, then a
  // PermissionRequest (the dialog appeared) that blocks until the server's
  // browser-decision wait times out (shrunk in the harness), a scripted
  // pause, then the tool_result + reply + stop.
  await startNewSession(page, 'permission-dialog ask before the tool');

  // The dialog notice appears while the question is pending in the TUI.
  await expect(page.getByTestId('permission-notice')).toBeVisible();

  // No browser decision: the hook waits out the decision deadline and passes
  // through to the TUI. The scripted tool_result then lands: the notice
  // resolves without any browser interaction, and the turn completes normally
  // (prompt, tool call with its result absorbed inline, closing reply). The
  // generous timeout covers the deliberate decision-deadline stall.
  await expect(page.getByTestId('permission-notice')).toHaveCount(0, {
    timeout: 15_000,
  });
  await expect(page.getByTestId('message-item')).toHaveCount(3);
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});
