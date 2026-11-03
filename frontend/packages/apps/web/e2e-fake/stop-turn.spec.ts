import { test, expect } from './support/fixtures';
import { sendMessage, startNewSession } from './support/app';

/**
 * "Stop" in the In progress strip stops a running Claude turn
 * (`POST /api/sessions/{id}/stop-turn`) — through the real loop.
 *
 * The case it must recover: the user presses Escape in the pane after the
 * prompt submitted but before any reply streamed. Claude Code drops the
 * prompt and leaves no trace: no `Stop` hook, no `[Request interrupted by
 * user]` marker. Without Stop the turn stays in flight in Delta forever, and
 * a send composed afterwards waits `queued` behind it.
 *
 * Scenario `stop-turn`: the fake fires `UserPromptSubmit` for the first prompt
 * and writes its user line, then writes nothing and fires no `Stop` — a turn
 * that never ends on its own. A second send queues behind it. Clicking Stop
 * makes Delta inject Escape into the pane, which the fake consumes writing
 * nothing (`await_escape`), exactly like the real TUI when no reply had
 * streamed — so only Delta's own turn end clears the strip. The Ctrl+C that
 * follows empties the fake's composer, and the queued send is then typed and
 * answered.
 */
test('stopping a turn that never ends on its own clears the spinner and dispatches the queued send', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'stop-turn this prompt never gets a reply');

  // The prompt submitted and its user line landed, but no reply and no Stop
  // ever follow: the turn stays in progress.
  const pending = page.getByTestId('pending-item');
  await expect(page.getByTestId('message-item')).toHaveCount(1, {
    timeout: 15_000,
  });
  await expect(pending).toHaveCount(1);
  await expect(page.getByRole('status', { name: 'in progress' })).toBeVisible();

  // A send composed now waits behind the running turn.
  await sendMessage(page, 'queued behind the running turn');
  await expect(pending).toHaveCount(2, { timeout: 15_000 });
  await expect(page.getByText('queued — sends when idle')).toBeVisible();

  const stop = page.getByRole('button', { name: 'Stop', exact: true });
  await expect(stop).toBeVisible();
  await stop.click();

  // The queued send left `queued`: it was typed into the pane after the
  // Escape, echoed, and answered — and with that turn complete the strip and
  // its spinner clear.
  await expect(page.getByText('queued — sends when idle')).toHaveCount(0, {
    timeout: 15_000,
  });
  await expect(
    page.getByText('answering the queue the stop released'),
  ).toBeVisible({ timeout: 15_000 });
  await expect(pending).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByRole('status', { name: 'in progress' })).toHaveCount(0);
});
