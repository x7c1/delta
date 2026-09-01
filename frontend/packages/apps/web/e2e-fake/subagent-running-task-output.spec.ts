import { test, expect, type Page } from './support/fixtures';
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
 * A BACKGROUND subagent whose result the parent RETRIEVES itself, with a
 * blocking `TaskOutput` call. Claude Code injects a `<task-notification>` only
 * for a completion the parent did not ask for, so no notification ever fires
 * here — and before the retrieval was folded as a completion, nothing else
 * could clear the entry: the turn-end sweep deliberately keeps background
 * entries, so the navigator spinner spun forever and the persisted launch row
 * kept re-seeding it on every reload.
 *
 * Scenario `subagent-running-task-output`: the fake fires `PreToolUse` for an
 * `Agent` with `run_in_background: true`, fires its immediate `PostToolUse`
 * (whose `tool_response.agentId` is the launch's task id), replies, and stops.
 * On the next prompt it writes a `TaskOutput` `tool_use` naming that task id
 * and the retrieval's successful `<status>completed</status>` result —
 * deliberately with NO `task_notification` step anywhere in the scenario. The
 * server correlates the retrieval by task id and clears the indicator.
 */
test('a background subagent finishes when the parent retrieves its result with TaskOutput', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(
    page,
    'subagent-running-task-output please run a background subagent',
  );

  const indicator = page.getByTestId('subagent-running-indicator');
  await expect(indicator).toContainText('Subagent running', { timeout: 15_000 });
  await expect(indicator).toContainText(
    'Probe in the background and hand back the result',
  );
  await expect(focusedRowRunning(page)).toBeVisible();

  // The launching turn ends but the background subagent keeps running.
  await expect(page.getByText('Launched the background subagent.')).toBeVisible({
    timeout: 15_000,
  });
  await expect(indicator).toBeVisible();
  await expect(focusedRowRunning(page)).toBeVisible();

  // A follow-up prompt drives the turn in which the fake retrieves the result.
  // Folding that retrieval finishes the subagent, so the indicator + spinner
  // clear with no notification involved.
  await sendMessage(page, 'any news?');
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(focusedRowRunning(page)).toHaveCount(0);
});
