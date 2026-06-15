import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

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
