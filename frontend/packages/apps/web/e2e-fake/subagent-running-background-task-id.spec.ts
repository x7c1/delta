import { test, expect, type Page } from '@playwright/test';
import { startNewSession, sendMessage } from './support/app';

/**
 * The running spinner on THIS test's session row. Scoped to the focused row
 * (`aria-current="true"`): the fake-mode suite shares one delta-server, so
 * earlier specs can leave other sessions running, and a bare `session-running`
 * would match more than one row.
 */
function focusedRowRunning(page: Page) {
  return page
    .locator('[data-testid="session-node"][aria-current="true"]')
    .getByTestId('session-running');
}

/**
 * A BACKGROUND subagent whose completion `<task-notification>` body is missing
 * `<tool-use-id>` — only `<task-id>` is present. Recent Claude Code versions
 * sometimes strip `<tool-use-id>` from the user-message notification body, and
 * before the task-id fallback the server only correlated on `<tool-use-id>`,
 * so `finish_subagent` never fired and the navigator's running indicator hung
 * forever.
 *
 * Scenario `subagent-running-background-task-id`: the fake fires `PreToolUse`
 * for an `Agent` whose input carries `run_in_background: true`, fires its
 * immediate `PostToolUse` (whose `tool_response.agentId` is the launch's task
 * id), replies, and stops. On the next prompt the fake writes a
 * `<task-notification>` line that deliberately OMITS `<tool-use-id>`, leaving
 * only `<task-id>`. The matching launch row was upgraded with `task_id` at the
 * PostToolUse hook, so the fallback correlation finishes the subagent and the
 * running spinner clears — the regression this fixes.
 */
test('a background subagent finishes via the task-id fallback when tool-use-id is dropped', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(
    page,
    'subagent-running-background-task-id please run a background subagent',
  );

  const indicator = page.getByTestId('subagent-running-indicator');
  await expect(indicator).toContainText('Subagent running', { timeout: 15_000 });
  await expect(indicator).toContainText('Probe in the background by task-id');
  await expect(focusedRowRunning(page)).toBeVisible();

  // The launching turn ends but the background subagent keeps running, like
  // the regular background spec.
  await expect(page.getByText('Launched the background subagent.')).toBeVisible({
    timeout: 15_000,
  });
  await expect(indicator).toBeVisible();
  await expect(focusedRowRunning(page)).toBeVisible();

  // A follow-up prompt drives the turn in which the fake writes the
  // tool-use-id-less `<task-notification>`. With the task-id fallback the
  // server still finishes the subagent and the indicator + spinner clear.
  await sendMessage(page, 'any news?');
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(focusedRowRunning(page)).toHaveCount(0);
});
