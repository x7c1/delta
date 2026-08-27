import { test, expect } from './support/fixtures';
import { scenarioPath } from './support/server';
import { startNewCodexSession } from './support/app';

/**
 * A new **Codex** session is the user's the moment the server accepts its first
 * send — not when its adapter has connected.
 *
 * The terminal-less twin of `slow-start.spec.ts`, and the proof that the
 * accept→launch split is one behaviour rather than a Claude-only one. A Codex
 * launch used to run inside `POST /api/sends`: the worktree checkout, spawning
 * `codex app-server`, its handshake and `thread/start` all completed before the
 * `201`, so a session started from a PR left the user on the new-session screen
 * for the whole thing.
 *
 * Scenario `codex-slow-start`: the fake app-server stalls its `thread/start`
 * response by 2.5 s, stretching the window between "the POST returned" and "the
 * session is live" wide enough to observe. Inside it the workspace must already
 * be ON the new session — the row is listed as `spawning`, so its card reads
 * `Starting`, its composer says the session is starting and offers no send, and
 * its first prompt sits in the pending strip. When `thread/start` finally
 * answers the same session becomes `Open` and the scripted reply arrives: one
 * continuous session, no hand-off the user can see.
 *
 * The Codex scenario is a server-wide setting, so this spec runs its own server
 * generation and restores the suite's shared one afterwards (see
 * `ServerHandle.restart`).
 */

test.afterEach(async ({ server }) => {
  // Restore the shared Codex scenario even when the test failed, so this spec's
  // delayed handshake cannot leak into the Codex specs that follow — they wait
  // for output at the default timeout.
  await server.restart();
});

test('a slow Codex launch is focused as a starting session, then comes up in place', async ({
  page,
  server,
}) => {
  await server.restart({
    FAKE_CODEX_SCENARIO: scenarioPath('codex-slow-start'),
  });

  await page.goto('/');
  await startNewCodexSession(page, 'wake up eventually');

  // Inside the delayed-`thread/start` window. Each assertion below must hold
  // BEFORE the fake answers, so they are kept tight and ordered cheapest first;
  // the scenario's 2.5 s is the budget for all of them.
  const startingCard = page
    .locator('li')
    .filter({ has: page.getByRole('status', { name: 'Starting', exact: true }) });
  await expect(startingCard).toHaveCount(1, { timeout: 2_000 });
  // The new-session screen is behind us: this is the spawned session's screen.
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0);
  const textbox = page.getByRole('textbox');
  await expect(textbox).toHaveAttribute(
    'placeholder',
    'This session is starting…',
  );
  // A starting session was never closed, and no send resumes it, so the closed
  // notice must stay off it — it would contradict the placeholder.
  await expect(page.getByTestId('readonly-notice')).toHaveCount(0);

  // The first prompt is visible exactly once while it waits. It is a `queued`
  // send row here (a Codex prompt cannot ride on a launch command line), which
  // is the same thing the strip renders.
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('wake up eventually');
  // …and the row says what it is actually waiting for. The ordinary queued
  // label ("sends when idle") describes a session busy with something else;
  // this one is not up yet.
  await expect(pending).toContainText('queued — sends when the session starts');

  // A follow-up cannot be sent yet: the server would refuse it
  // (`409 session_spawning`), so the composer does not offer one — even with a
  // draft ready to go.
  await textbox.fill('a follow-up, once you are up');
  await expect(page.getByRole('button', { name: 'Send' })).toBeDisabled();

  // `thread/start` answers: the very same card flips to Open — no second
  // session, no return to the new-session screen — and the held first prompt is
  // dispatched, so the scripted reply arrives.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1, { timeout: 15_000 });
  await expect(
    page.getByRole('status', { name: 'Starting', exact: true }),
  ).toHaveCount(0);
  // The prompt and the scripted answer, in that order — asserted by count, so
  // the spec never depends on what the scenario replies (see e2e.md).
  await expect(page.getByTestId('message-item')).toHaveCount(2, {
    timeout: 15_000,
  });
  // And the composer is live again, with the draft that was held back ready to
  // send — the same textarea, never reset by the hand-off.
  await expect(textbox).toHaveValue('a follow-up, once you are up');
  await expect(page.getByRole('button', { name: 'Send' })).toBeEnabled();
});
