import { test, expect } from '@playwright/test';
import { startNewSession, sendMessage } from './support/app';

/**
 * A running subagent (the `Agent`/`Task` tool) is surfaced while it works and
 * cleared when it completes — the foreground `PreToolUse(Agent)` →
 * `PostToolUse(Agent)` window.
 *
 * Scenario `subagent-running`: the fake fires `PreToolUse` for an `Agent` tool
 * call (carrying `subagent_type` and `description`), holds the turn open so the
 * running window is observable, then fires `PostToolUse` for the same
 * `tool_use_id`, writes the tool_result, replies, and stops.
 *
 * A subagent runs in its own transcript Delta never tails, so nothing else
 * appears in the conversation pane while it works. The spec asserts:
 *
 * 1. While the subagent runs, the conversation pane shows the running indicator
 *    with the subagent's description, and the navigator row shows the subagent
 *    badge.
 * 2. Once the subagent finishes (`PostToolUse`), both the indicator and the
 *    badge are gone.
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

  // The navigator row also carries a dedicated subagent badge.
  await expect(page.getByTestId('session-subagent-badge')).toBeVisible();

  // Once the subagent completes (PostToolUse), the indicator and the badge are
  // both gone.
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByTestId('session-subagent-badge')).toHaveCount(0);
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
 * The spec asserts the indicator/badge appear at launch, SURVIVE the turn end
 * (the distinguishing behaviour from the foreground case), and clear only after
 * the completion notification.
 */
test('a background subagent survives the launching turn and clears on its completion notification', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'subagent-running-background please run a background subagent');

  // While the background subagent runs, the conversation pane shows the running
  // indicator and the navigator row shows the badge.
  const indicator = page.getByTestId('subagent-running-indicator');
  await expect(indicator).toContainText('Subagent running', { timeout: 15_000 });
  await expect(indicator).toContainText('Probe the codebase in the background');
  await expect(page.getByTestId('session-subagent-badge')).toBeVisible();

  // The launching turn has ended (the fake replied and stopped after the
  // immediate PostToolUse), yet the background subagent's indicator and badge
  // SURVIVE — the distinguishing behaviour from a foreground subagent.
  await expect(page.getByText('Launched the background subagent.')).toBeVisible({
    timeout: 15_000,
  });
  await expect(indicator).toBeVisible();
  await expect(page.getByTestId('session-subagent-badge')).toBeVisible();

  // A follow-up prompt drives the turn in which the fake writes the completion
  // `<task-notification>`; folding it finishes the background subagent, so the
  // indicator and badge finally clear.
  await sendMessage(page, 'any news?');
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByTestId('session-subagent-badge')).toHaveCount(0);
});
