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
 * A modern `Agent` tool_use with NO `run_in_background` key in its input.
 * Modern Claude Code dropped that parameter from the `Agent`/`Task` tool
 * schema and made these calls async by default, so production transcripts now
 * carry `tool_use(Agent)` blocks with no flag at all. Before the predicate fix
 * the parent-transcript fold treated the missing flag as foreground, so the
 * immediate `PostToolUse(Agent)` (which fires at launch, not at completion)
 * closed the running window within milliseconds and the navigator never
 * showed the indicator — the bug this spec pins.
 *
 * Scenario `subagent-running-implicit-background`: the fake fires `PreToolUse`
 * for an `Agent` whose input carries `subagent_type`, `description`, and
 * `prompt` but NO `run_in_background` key, then its immediate `PostToolUse`,
 * replies, and stops — the launching turn ends while the implicit-background
 * subagent keeps running. On the next prompt the fake writes the
 * `<task-notification>` completion line, which the server folds to finish the
 * subagent.
 *
 * The spec asserts the indicator and the running spinner appear at launch,
 * SURVIVE both the immediate PostToolUse and the turn end (the bug was that
 * they did not survive even milliseconds), and clear only after the completion
 * notification.
 */
test('an Agent tool_use with no run_in_background flag survives PostToolUse and the turn ending', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(
    page,
    'subagent-running-implicit-background please run an implicit-background subagent',
  );

  // While the implicit-background subagent runs, the conversation pane shows
  // the running indicator labelled with the subagent's description, and the
  // navigator row shows the running spinner.
  const indicator = page.getByTestId('subagent-running-indicator');
  await expect(indicator).toContainText('Subagent running', { timeout: 15_000 });
  await expect(indicator).toContainText('Probe in the implicit background');
  await expect(focusedRowRunning(page)).toBeVisible();

  // The launching turn has ended (the fake replied and stopped after the
  // immediate PostToolUse). The indicator and the running spinner SURVIVE the
  // turn end — this is the regression the predicate fix introduced: before the
  // fix the immediate PostToolUse cleared the running set within milliseconds.
  await expect(
    page.getByText('Launched the implicit-background subagent.'),
  ).toBeVisible({ timeout: 15_000 });
  await expect(indicator).toBeVisible();
  await expect(focusedRowRunning(page)).toBeVisible();

  // A follow-up prompt drives the turn in which the fake writes the completion
  // `<task-notification>`; folding it finishes the implicit-background
  // subagent, so the indicator and the running spinner finally clear.
  await sendMessage(page, 'any news?');
  await expect(indicator).toHaveCount(0, { timeout: 15_000 });
  await expect(focusedRowRunning(page)).toHaveCount(0);
});
