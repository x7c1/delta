import { test, expect } from './support/fixtures';
import { sendMessage, startNewSession } from './support/app';

/**
 * The worktree `./scenarios/relocate.json` enters — this spec's single point
 * of coupling to the fake. The fake makes it the session's new working
 * directory, and every transcript line written after the move carries it as
 * its `cwd`, so the reply footer naming it proves the footer came from a line
 * read out of the moved transcript.
 */
const WORKTREE_DIR = 'relocated-wt';

/**
 * A session keeps flowing after Claude Code relocates its transcript.
 *
 * Scenario `relocate`: the fake calls `EnterWorktree` and then does what
 * Claude Code does — it moves the transcript to the worktree's project
 * directory (the old file is gone), appends a `relocated` line, and reports
 * the new path from the tool's `PostToolUse` on. The reply it writes after the
 * move, and the turn's `Stop`, must reach the browser: the reply lands with its
 * footer and the pending chip drains. A second turn, whose prompt is reported
 * with the new path too, flows the same way.
 */
test('replies written after the transcript moves appear with their footer', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'relocate into a worktree');

  const pending = page.getByTestId('pending-item');
  const messages = page.getByTestId('message-item');
  const latestMeta = page.locator('[data-testid="message-meta"][data-latest="true"]');

  // The post-move reply is ingested and the turn completes.
  await expect(pending).toHaveCount(0, { timeout: 15_000 });
  await expect(latestMeta).toHaveCount(1);
  await expect(latestMeta.getByTestId('meta-cwd')).toContainText(WORKTREE_DIR);
  await expect(latestMeta.getByTestId('meta-time')).toBeVisible();
  const afterFirstTurn = await messages.count();

  // The next turn is reported entirely against the moved transcript: its
  // prompt and reply both land, and the chip drains again.
  await sendMessage(page, 'and another turn');
  await expect(messages).toHaveCount(afterFirstTurn + 2, { timeout: 15_000 });
  await expect(pending).toHaveCount(0);
  await expect(latestMeta).toHaveCount(1);
  await expect(latestMeta.getByTestId('meta-cwd')).toContainText(WORKTREE_DIR);
});
