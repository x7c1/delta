import { test, expect } from './support/fixtures';
import { startNewSession, sendMessage } from './support/app';

/**
 * Cancelling a `dispatched` send whose echo never arrived: the user pressed
 * Escape in the TUI to discard the composer buffer, leaving the row stuck
 * `dispatched` forever (the regression). Clicking Cancel in the browser
 * reproduces the keypress on their behalf — the server injects Escape into
 * the pane, drops the row to `cancelled`, and the next send proceeds
 * normally.
 *
 * Scenario `cancel-dispatched-send`: the fake answers the positional first
 * prompt (`reply` + `stop`) so the session reaches its first idle state.
 * The follow-up send goes through `send_line` and lands in the fake's
 * `await_escape` step, which DROPS the typed prompt and BLOCKS for Escape —
 * so no `UserPromptSubmit` ever fires and the row is held in `AwaitingEcho`
 * indefinitely, exactly like the real TUI when the composer buffer is
 * discarded. Cancelling injects Escape; the fake's `await_escape` returns
 * and the scenario advances to `await_prompt`, which consumes the next
 * follow-up send and answers it.
 *
 * The spec asserts: the dispatched chip shows up, the Cancel button on
 * that chip clears the strip, and a subsequent send completes — i.e. the
 * composer is no longer locked.
 */
test('cancelling a dispatched send injects Escape and unblocks the next send', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'cancel-dispatched-send opening prompt');

  // The positional first prompt is auto-submitted, the fake replies and
  // stops. Wait for the reply so we know the session is idle before the
  // dispatched-cancel step.
  await expect(page.getByText('session opened')).toBeVisible({
    timeout: 15_000,
  });

  // Send a follow-up. The fake's next step is `await_escape`, which DROPS
  // the typed prompt without firing `UserPromptSubmit` — modelling the user
  // pressing Escape in the TUI to discard the composer buffer before the
  // submit landed. The row therefore stays `dispatched` indefinitely.
  await sendMessage(page, 'this will be cancelled');

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1, { timeout: 15_000 });
  // The dispatched chip shows the "awaiting reply" spinner alongside the
  // Cancel control — the user-visible escape hatch.
  await expect(page.getByText('awaiting reply')).toBeVisible();

  await page.getByRole('button', { name: 'Cancel' }).click();

  // The cancel injected Escape into the pane, marked the row `cancelled`,
  // and dropped the turn back to Idle. The strip clears.
  await expect(pending).toHaveCount(0, { timeout: 15_000 });

  // The composer is unlocked: a fresh send goes through normally and the
  // fake answers it.
  await sendMessage(page, 'follow-up message');
  await expect(page.getByText('follow-up answered')).toBeVisible({
    timeout: 15_000,
  });
});
