import { test, expect } from './support/fixtures';
import { startNewCodexSession } from './support/app';

/**
 * Several approvals outstanding at once — through the real loop, on the Codex
 * (adapter) path.
 *
 * A pane-backed provider cannot produce this: its permission hook blocks the CLI
 * until the dialog is answered, so at most one request is ever pending. An
 * adapter-backed one runs tool calls in parallel, and the scenario
 * `codex-parallel-approvals` re-enacts what a real `codex app-server` did in the
 * field: three `exec_command` approvals emitted back to back, with the turn
 * suspended until every one of them is answered.
 *
 * The failure this pins was silent and total. Delta mirrored the pending dialog
 * in a single slot, so each request overwrote the previous one: the browser only
 * ever showed the last, one Allow answered it, and the other two waited forever
 * — the turn stayed in progress with no dialog on screen and nothing for the user
 * to act on. So this walks the queue: the FIRST request is on screen with the
 * remaining count next to it, each answer promotes the next without a refresh,
 * and the turn completes only after the last one.
 */

test('a parallel approval fan-out surfaces one dialog at a time until the turn completes', async ({
  page,
}) => {
  await page.goto('/');
  await startNewCodexSession(page, 'read three files at once');

  const notice = page.getByTestId('permission-notice');
  const remaining = page.getByTestId('permission-notice-remaining');

  // The oldest request owns the card, and the card says two more are waiting —
  // so the user knows the queue is not empty before answering anything.
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('cat alpha.txt');
  await expect(remaining).toContainText('+2 more');

  // Answering promotes the next request into the same card. No refetch, no
  // reload: the dialog is never absent while approvals are pending.
  await notice.getByRole('button', { name: 'Allow', exact: true }).click();
  await expect(notice).toContainText('cat beta.txt');
  await expect(remaining).toContainText('+1 more');

  await notice.getByRole('button', { name: 'Allow', exact: true }).click();
  await expect(notice).toContainText('cat gamma.txt');
  // The last one: nothing is queued behind it, so no count is shown.
  await expect(remaining).toHaveCount(0);

  // With every approval answered the provider finishes the turn — the deadlock
  // is gone: the notice clears, the reply lands, and nothing is left pending.
  await notice.getByRole('button', { name: 'Allow', exact: true }).click();
  await expect(notice).toHaveCount(0);
  await expect(page.getByText('all three files read')).toBeVisible();
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});
