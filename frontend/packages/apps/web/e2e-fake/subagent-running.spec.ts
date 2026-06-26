import { test, expect, type Page } from '@playwright/test';
import { startNewSession, sendMessage } from './support/app';

/**
 * The running spinner on THIS test's session row. Scoped to the focused row
 * (`aria-current="true"`): the fake-mode suite shares one delta-server, so
 * earlier specs can leave other sessions running, and a bare `session-running`
 * would match more than one row. Only the session started here is focused.
 */
function focusedRowRunning(page: Page) {
  return page
    .locator('[data-testid="session-node"][aria-current="true"]')
    .getByTestId('session-running');
}

/**
 * A running subagent (the `Agent`/`Task` tool) is surfaced while it works and
 * cleared when it completes — the foreground `PreToolUse(Agent)` →
 * `PostToolUse(Agent)` window.
 *
 * Scenario `subagent-running`: the fake fires `PreToolUse` for an `Agent` tool
 * call (carrying `subagent_type`, `description`, and an explicit
 * `run_in_background: false` — required for foreground semantics now that
 * modern Claude Code makes `Agent`/`Task` calls async by default), holds the
 * turn open so the running window is observable, then fires `PostToolUse` for
 * the same `tool_use_id`, writes the tool_result, replies, and stops.
 *
 * A subagent runs in its own transcript Delta never tails, so nothing else
 * appears in the conversation pane while it works. The spec asserts:
 *
 * 1. While the subagent runs, the conversation pane shows the running indicator
 *    with the subagent's description, and the navigator row shows the running
 *    spinner (a running subagent folds into the row's running state, so the
 *    spinner alone signals "still working" — there is no separate badge).
 * 2. Once the subagent finishes (`PostToolUse`) and the turn ends, both the
 *    indicator and the spinner are gone.
 */
test('a running subagent is shown while it works and cleared when it finishes', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'subagent-running please run a subagent');

  // While the subagent runs, the conversation pane shows the running indicator
  // labelled with the subagent's description.
  const indicator = page.getByTestId('subagent-running-indicator');
  await expect(indicator).toContainText('Subagent running', {
    timeout: 15_000,
  });
  await expect(indicator).toContainText('Probe the codebase');

  // The navigator row shows the running spinner while the subagent works.
  await expect(focusedRowRunning(page)).toBeVisible();

  // Once the subagent completes (PostToolUse) and the turn ends, the indicator
  // and the running spinner are both gone.
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(focusedRowRunning(page)).toHaveCount(0);
});

/**
 * A BACKGROUND subagent (`run_in_background: true`) survives the launching turn
 * and is cleared only by its completion notification — not by its immediate
 * `PostToolUse` nor by the turn ending.
 *
 * Scenario `subagent-running-background`: the fake fires `PreToolUse` for an
 * `Agent` whose input carries `run_in_background: true`, then its immediate
 * `PostToolUse` (the launch returned), replies, and stops — the launching turn
 * ends while the background subagent keeps running. On the next prompt the fake
 * writes the `<task-notification>` completion line, which the server folds to
 * finish the subagent.
 *
 * The spec asserts the indicator and the running spinner appear at launch,
 * SURVIVE the turn end (the distinguishing behaviour from the foreground case —
 * the background subagent keeps the row running past its launching turn), and
 * clear only after the completion notification.
 */
test('a background subagent survives the launching turn and clears on its completion notification', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'subagent-running-background please run a background subagent');

  // While the background subagent runs, the conversation pane shows the running
  // indicator and the navigator row shows the running spinner.
  const indicator = page.getByTestId('subagent-running-indicator');
  await expect(indicator).toContainText('Subagent running', { timeout: 15_000 });
  await expect(indicator).toContainText('Probe the codebase in the background');
  await expect(focusedRowRunning(page)).toBeVisible();

  // The launching turn has ended (the fake replied and stopped after the
  // immediate PostToolUse), yet the background subagent's indicator and the
  // running spinner SURVIVE — the distinguishing behaviour from a foreground
  // subagent (the running subagent keeps the row running past its turn).
  await expect(page.getByText('Launched the background subagent.')).toBeVisible({
    timeout: 15_000,
  });
  await expect(indicator).toBeVisible();
  await expect(focusedRowRunning(page)).toBeVisible();

  // A follow-up prompt drives the turn in which the fake writes the completion
  // `<task-notification>`; folding it finishes the background subagent, so the
  // indicator and the running spinner finally clear.
  await sendMessage(page, 'any news?');
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(focusedRowRunning(page)).toHaveCount(0);
});
