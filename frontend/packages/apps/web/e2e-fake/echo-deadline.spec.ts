import { test, expect } from './support/fixtures';
import { startNewSession, sendMessage } from './support/app';
import { fetchSends, latestSession } from './support/rest';

/**
 * A send whose keystrokes are swallowed with NO signal at all is retried once
 * and then parked — never left "in progress" forever, and never lost.
 *
 * The production incident: Claude Code raised its own interactive dialog
 * between turns, the dispatched send's pasted text was swallowed whole, and its
 * trailing Enter answered the dialog instead of submitting a prompt. No user
 * message, no `UserPromptSubmit`, no turn boundary — so every event-driven
 * recovery had nothing to react to, and the row stayed `dispatched` behind a
 * permanent "In progress" with the next send queued behind it.
 *
 * Parking is what ends that wait, and it keeps the message: the row goes back
 * to `queued` with `held_at` set — the same held state the boot restore
 * produces — so it stays in the pending strip with explicit Send and Cancel
 * controls and never auto-dispatches. The `send_parked` notice only explains
 * why it is waiting.
 *
 * Scenario `echo-deadline`: the fake answers the positional first prompt
 * (`reply` + `stop`), then `swallow_prompt` TWICE. The first swallow eats the
 * send; the echo-deadline watchdog releases the turn, injects `Escape`, and
 * re-types it; the second swallow eats the retry too, so the second deadline
 * spends the budget and parks the send. The fake's remaining two blocks answer
 * the released message and the user's next message normally.
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

test('a send swallowed without a trace is retried once, then parked in the queue, and released on demand', async ({
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
  const session = await latestSession(page);

  // Send the message the dialog eats. The fake consumes the keystrokes and
  // fires nothing at all, so the row is `dispatched` behind a missing echo —
  // the state that used to be terminal.
  await sendMessage(page, 'the message a dialog swallowed');

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1, { timeout: 15_000 });

  // First deadline: Escape + re-type. The second `swallow_prompt` eats that
  // too, so the second deadline parks the send — the notice explains why, and
  // the message itself stays in the queue instead of vanishing.
  const parked = page.getByTestId('send-parked-notice');
  await expect(parked).toBeVisible({ timeout: 20_000 });

  // Server-side truth, not just the chip: the row is `queued` with the hold
  // marker, so nothing will dispatch it on its own.
  await expect(async () => {
    const sends = await fetchSends(page, session.id);
    expect(sends.sends).toHaveLength(1);
    expect(sends.sends[0].status).toBe('queued');
    expect(sends.sends[0].held_at).not.toBeNull();
    expect(sends.turn.state).toBe('idle');
  }).toPass({ timeout: 20_000 });

  // The held row carries the neutral label and both explicit controls — the
  // user decides whether the swallowed message goes out again.
  const heldRow = pending.filter({ hasText: 'the message a dialog swallowed' });
  await expect(heldRow).toHaveCount(1, { timeout: 20_000 });
  await expect(heldRow.getByText('Held — send or cancel')).toBeVisible();
  await expect(heldRow.getByRole('button', { name: 'Cancel' })).toBeVisible();
  const releaseButton = heldRow.getByRole('button', { name: 'Send' });
  await expect(releaseButton).toBeVisible();
  // Nothing is running: the parked row is waiting, not in flight.
  await expect(page.getByTestId('session-running')).toHaveCount(0);

  // Press Send: the release types the message once — this time the fake takes
  // it — and the strip drains.
  await releaseButton.click();
  await expect(page.getByText('released message answered')).toBeVisible({
    timeout: 20_000,
  });
  await expect(pending).toHaveCount(0, { timeout: 20_000 });
  // And the notice goes with the row it pointed at: it asked the user to send
  // or cancel, and they sent — leaving it up would keep explaining a row that
  // is no longer in the queue.
  await expect(parked).toHaveCount(0, { timeout: 20_000 });
  await expect(async () => {
    expect((await fetchSends(page, session.id)).sends).toHaveLength(0);
  }).toPass({ timeout: 20_000 });

  // The composer is not wedged: the next send goes through the normal path and
  // the fake answers it.
  await sendMessage(page, 'follow-up message');
  await expect(page.getByText('follow-up answered')).toBeVisible({
    timeout: 15_000,
  });
  await expect(pending).toHaveCount(0, { timeout: 15_000 });
});
