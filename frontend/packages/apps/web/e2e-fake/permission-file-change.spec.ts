import { test, expect } from './support/fixtures';
import { scenarioPath } from './support/server';
import { startNewCodexSession } from './support/app';

/**
 * A file-change approval, through the real loop, showing what it would write.
 *
 * The failure this pins: `item/fileChange/requestApproval` carries only
 * `{ itemId, startedAtMs, threadId, turnId, grantRoot?, reason? }`, so the card
 * had nothing to show but that blob of ids, truncated. A dogfooding turn raised
 * thirteen of them in a row — thirteen identical, unanswerable prompts. The
 * paths and diffs had in fact crossed the wire a moment earlier, on the
 * `item/started` for the same item, and were dropped because nothing joined the
 * two.
 *
 * What this proves that a unit test cannot: the join survives every hop between
 * the app-server and the browser — the adapter's correlation, the neutral event,
 * the wire frame, the store, the card.
 *
 * Scenario `codex-file-change-approval` (this spec's own): one `fileChange` item
 * announcing a one-file patch, then the approval that gates it, with the turn
 * parked until it is answered. The Codex scenario is a server-wide setting, so
 * this spec runs its own server generation and restores the suite's shared one
 * afterwards (see `ServerHandle.restart`).
 */

test.afterEach(async ({ server }) => {
  // Restore the shared Codex scenario even when the test failed, so this spec's
  // turn cannot leak into the Codex specs that follow.
  await server.restart();
});

test('a file-change approval names the file it would change and shows the diff on expand', async ({
  page,
  server,
}) => {
  await server.restart({
    FAKE_CODEX_SCENARIO: scenarioPath('codex-file-change-approval'),
  });

  await page.goto('/');
  await startNewCodexSession(page, 'update the greeting');

  const notice = page.getByTestId('permission-notice');
  await expect(notice).toBeVisible({ timeout: 15_000 });

  // The affected file and how it changes are on screen without any interaction:
  // this is the fact the answer turns on.
  await expect(notice.getByText('src/greeting.rs')).toBeVisible();
  await expect(notice.getByText('edit')).toBeVisible();
  await expect(notice.getByTestId('permission-notice-reason')).toHaveText(
    'write access to the worktree',
  );

  // And the blob it replaced is gone — not shown alongside it.
  await expect(notice.getByText(/itemId/)).toHaveCount(0);

  // The diff is one click away, not inline: a real patch can be hundreds of
  // lines, and burying Allow/Deny under it makes the common answer harder.
  await expect(notice.getByText(/\+fn greet/)).toHaveCount(0);
  await notice.getByRole('button', { name: /Diff \(1 file\)/ }).click();
  await expect(notice.getByText(/\+fn greet/)).toBeVisible();

  // The decision still reaches the provider: the parked turn finishes.
  await notice.getByRole('button', { name: 'Allow', exact: true }).click();
  await expect(notice).toHaveCount(0);
  await expect(page.getByText('the greeting was updated')).toBeVisible({
    timeout: 15_000,
  });
});
