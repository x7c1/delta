import { test, expect } from './support/fixtures';
import { startNewSession, sendMessage } from './support/app';

/**
 * A send whose keystrokes are swallowed with NO signal at all is retried once
 * and then parked — never left "in progress" forever.
 *
 * The production incident: Claude Code raised its own interactive dialog
 * between turns, the dispatched send's pasted text was swallowed whole, and its
 * trailing Enter answered the dialog instead of submitting a prompt. No user
 * message, no `UserPromptSubmit`, no turn boundary — so every event-driven
 * recovery had nothing to react to, and the row stayed `dispatched` behind a
 * permanent "In progress" with the next send queued behind it.
 *
 * Scenario `echo-deadline`: the fake answers the positional first prompt
 * (`reply` + `stop`), then `swallow_prompt` TWICE. The first swallow eats the
 * send; the echo-deadline watchdog releases the turn, injects `Escape`, and
 * re-types it; the second swallow eats the retry too, so the second deadline
 * spends the budget and parks the send. The fake's following `await_prompt`
 * then serves the user's next message normally.
 *
 * The watchdog deadline is server-wide, so this spec runs its own server
 * generation with a short one and restores the suite's value afterwards (see
 * `ServerHandle.restart`).
 */

/** The shortened echo deadline this spec's server generation runs with. */
const ECHO_DEADLINE_MS = '3000';

test.afterEach(async ({ server }) => {
  // Restore the shared configuration even when the test failed, so the short
  // deadline cannot leak into the specs that follow.
  await server.restart();
});

test('a send swallowed without a trace is retried once, then parked, and the next send flows', async ({
  page,
  server,
}) => {
  await server.restart({ DELTA_ECHO_DEADLINE_MS: ECHO_DEADLINE_MS });

  await page.goto('/');
  await startNewSession(page, 'echo-deadline opening prompt');

  // The positional first prompt is auto-submitted; the fake replies and stops,
  // so the session is idle before the swallowed send.
  await expect(page.getByText('session opened')).toBeVisible({
    timeout: 15_000,
  });

  // Send the message the dialog eats. The fake consumes the keystrokes and
  // fires nothing at all, so the row is `dispatched` behind a missing echo —
  // the state that used to be terminal.
  await sendMessage(page, 'the message a dialog swallowed');

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1, { timeout: 15_000 });

  // First deadline: Escape + re-type. The second `swallow_prompt` eats that
  // too, so the second deadline parks the send — the chip drains and the
  // browser is handed the text back with an explanation, instead of the
  // message vanishing.
  const parked = page.getByTestId('send-parked-notice');
  await expect(parked).toBeVisible({ timeout: 20_000 });
  await expect(parked).toContainText('the message a dialog swallowed');
  await expect(pending).toHaveCount(0, { timeout: 20_000 });

  // The composer is not wedged: the next send goes through the normal path and
  // the fake answers it.
  await sendMessage(page, 'follow-up message');
  await expect(page.getByText('follow-up answered')).toBeVisible({
    timeout: 15_000,
  });
  await expect(pending).toHaveCount(0, { timeout: 15_000 });
});
