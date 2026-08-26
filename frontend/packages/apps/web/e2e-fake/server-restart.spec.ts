import { test, expect } from './support/fixtures';
import { sendMessage, startNewSession } from './support/app';
import { fetchSends, latestSession } from './support/rest';

/**
 * Server restart with a `dispatched` send in flight — the full restore/release
 * saga end to end.
 *
 * The production incident: a send Delta had typed into the pane, still
 * `dispatched` (its `UserPromptSubmit` echo never arrived), when the server
 * process died. Turn state is runtime-only and rebuilds `Idle` on boot, but
 * the send *row* is persistent — so the boot sweep
 * (`SessionStore::restore_all_dispatched`) turns every orphaned `dispatched`
 * row back into `queued` with `held_at` set, and the row is deliberately
 * *never* auto-resent: stale text must not be re-submitted into a conversation
 * that has moved on. The user decides, via an explicit Send
 * (`POST /api/sends/{id}/release`) or Cancel.
 *
 * Scenario `server-restart`: the fake answers the positional first prompt
 * (`reply` + `stop`) so the session reaches idle, then `swallow_prompt`
 * consumes the follow-up send WITHOUT firing `UserPromptSubmit` (no echo) and
 * `hang`s — leaving the send `dispatched` behind a missing echo, exactly the
 * zombie shape the restart must recover. After the restart the session is
 * reopened through a fresh `claude --resume` pane, which the wrapper routes to
 * the fake's built-in echo loop (a resume carries no positional prompt) — it
 * awaits the released prompt, replies, and stops.
 *
 * The spec asserts the whole saga: the row is `dispatched` (over REST) before
 * the kill; after a SIGKILL + relaunch against the same DB/socket the client
 * reconnects, the send surfaces restored (badge + explicit Send/Cancel, and it
 * is NOT auto-resent), and pressing Send releases it through the resumed pane
 * to a completed turn. It leaves the relaunched server healthy so the rest of
 * the shared serial suite is unaffected.
 */
test('a dispatched send survives a server restart as a restored row and is released to completion', async ({
  page,
  server,
}) => {
  await page.goto('/');
  await startNewSession(page, 'server-restart opening prompt');

  // The positional first prompt is auto-submitted; the fake replies and stops,
  // so the session is idle before the dispatched-but-swallowed step.
  await expect(page.getByText('session opened')).toBeVisible({ timeout: 15_000 });
  const session = await latestSession(page);
  await expect(async () => {
    expect((await fetchSends(page, session.id)).turn.state).toBe('idle');
  }).toPass({ timeout: 15_000 });

  // Send a follow-up. The fake's next step is `swallow_prompt`, which reads the
  // typed prompt off stdin but fires no `UserPromptSubmit` — so the row stays
  // `dispatched` behind a missing echo, modelling the send that was in flight
  // when the process died.
  await sendMessage(page, 'this send was in flight at the crash');

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1, { timeout: 15_000 });

  // The precondition for the whole test: the server holds the send `dispatched`
  // (asserted over REST, not just the UI chip) at the moment we kill it.
  await expect(async () => {
    const sends = await fetchSends(page, session.id);
    expect(sends.sends).toHaveLength(1);
    expect(sends.sends[0].status).toBe('dispatched');
    expect(sends.sends[0].held_at).toBeNull();
  }).toPass({ timeout: 15_000 });

  // SIGKILL the server (a hard death) and relaunch it against the SAME
  // database, tmux socket, and claude wrapper. `restart()` polls `/health` in
  // the worker process and only resolves once the new generation is ready, so
  // no REST call below races the down window.
  await server.restart();

  // The client's live socket dropped when the server died and reconnects on
  // its own once it is back — the same `connection-indicator` wait the
  // ws-reconnect suite uses. The generous timeout covers the reconnect backoff.
  await expect(page.getByTestId('connection-indicator')).toHaveAttribute(
    'data-connection',
    'open',
    { timeout: 20_000 },
  );

  // The session reads as closed after the restart — turn state rebuilt `Idle` —
  // and the boot sweep recovered the orphaned `dispatched` row as a restored
  // `queued` send: still one open send, now carrying `held_at`.
  await expect(async () => {
    const sends = await fetchSends(page, session.id);
    expect(sends.turn.state).toBe('idle');
    expect(sends.sends).toHaveLength(1);
    expect(sends.sends[0].status).toBe('queued');
    expect(sends.sends[0].held_at).not.toBeNull();
  }).toPass({ timeout: 20_000 });

  // The restored send is NOT auto-resent: it surfaces as a queued row with the
  // neutral held badge and the explicit Send/Cancel controls, exactly once —
  // the user decides whether the stale text goes through. The badge names the
  // state rather than the cause, because the echo-deadline park leaves a row
  // in exactly this one (see `echo-deadline.spec.ts`).
  const restoredRow = pending.filter({ hasText: 'this send was in flight at the crash' });
  await expect(restoredRow).toHaveCount(1, { timeout: 20_000 });
  await expect(restoredRow.getByText('Held — send or cancel')).toBeVisible();
  const sendButton = restoredRow.getByRole('button', { name: 'Send' });
  await expect(sendButton).toBeVisible();
  await expect(restoredRow.getByRole('button', { name: 'Cancel' })).toBeVisible();
  // No turn is running: the restored row is waiting, not in flight.
  await expect(page.getByTestId('session-running')).toHaveCount(0);

  // Press Send: the release reopens the session through a fresh `claude
  // --resume` pane (a new pane token — surviving panes are never re-attached),
  // types the released prompt, and the resumed fake's echo loop answers it. The
  // pending strip drains and the turn completes.
  await sendButton.click();

  await expect(pending).toHaveCount(0, { timeout: 30_000 });
  await expect(async () => {
    const sends = await fetchSends(page, session.id);
    expect(sends.sends).toHaveLength(0);
    expect(sends.turn.state).toBe('idle');
  }).toPass({ timeout: 30_000 });
  // The released message was delivered as a user turn and answered: the
  // transcript grew past the pre-restart "session opened" turn.
  await expect(page.getByText('this send was in flight at the crash')).toBeVisible();
  await expect(page.getByTestId('session-running')).toHaveCount(0);
});
