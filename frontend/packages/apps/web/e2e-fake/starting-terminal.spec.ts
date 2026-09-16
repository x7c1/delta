import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * The embedded terminal is reachable while a session is still starting.
 *
 * The dogfooding failure this closes: a session started in a repository Claude
 * Code had never been trusted in left the pane on its workspace-trust dialog.
 * Nobody could answer it — the pane was sealed off until the session bound, and
 * it could not bind until somebody answered — so the launch sat there until the
 * watchdog killed it. The pane is the product's own escape hatch for exactly
 * that, so it has to be open during the one window where it is the only way out.
 *
 * Scenario `starting-attach`: the fake holds its `SessionStart` hook back by 8 s
 * while its banner is already on screen, so the "pane up, nothing bound" state —
 * a couple of hundred milliseconds in a healthy launch — is wide enough to drive
 * a browser through.
 *
 * The launch watchdog is server-wide and the shared suite shrinks it to 3 s, so
 * this spec runs its own server generation with a production-shaped deadline and
 * restores the suite's value afterwards (see `ServerHandle.restart`).
 */

/** The launch deadline this spec's server generation runs with. */
const LAUNCH_DEADLINE_MS = '30000';

test.afterEach(async ({ server }) => {
  // Restore the shared configuration even when the test failed, so the long
  // deadline cannot leak into the specs that follow.
  await server.restart();
});

test('the terminal attaches to a session that is still starting, and stays attached when it binds', async ({
  page,
  server,
}) => {
  await server.restart({ DELTA_LAUNCH_DEADLINE_MS: LAUNCH_DEADLINE_MS });

  await page.goto('/');
  await startNewSession(page, 'starting-attach hold the hook back');

  // Inside the delayed-hook window: the session is listed as starting, and
  // nothing has bound its pane.
  const starting = page.getByRole('status', { name: 'Starting', exact: true });
  await expect(starting).toHaveCount(1, { timeout: 5_000 });

  // The attach is observable: once the bridge's `tmux attach` client connects,
  // tmux redraws the pane into the browser terminal, and the fake's banner ends
  // with an identifying line that sits on the cursor row — in view however small
  // the fitted viewport is.
  await page.getByRole('button', { name: 'Terminal', exact: true }).click();
  const xtermInput = page.locator('.xterm-helper-textarea');
  await expect(xtermInput).toBeAttached();
  await expect(page.locator('.xterm-rows')).toContainText('fake-claude session');

  // All of that happened while the session was still starting — the point of
  // the spec — and it was never described as closed, which is what the pane
  // used to say here. "Resume it" is not something the user could have done.
  await expect(starting).toHaveCount(1);
  await expect(page.getByText('This session is closed.')).toHaveCount(0);

  // Stamp the live terminal's own element. `TerminalPane` tears an instance
  // down by removing its element and building a fresh one, so the stamp
  // surviving the bind is what makes "no detach, no reattach" an assertion that
  // can fail — a rebuilt terminal redraws the same banner, so the text below
  // cannot tell the two apart.
  await page
    .locator('.xterm')
    .first()
    .evaluate((el) => el.setAttribute('data-attached-before-bind', ''));

  // The hook lands: the same card flips to Open, and the terminal that was
  // attached before the bind is still the one on screen — no detach, no
  // reattach, nothing for the user to redo. A rebuild here would land on the
  // user answering the dialog they attached for, which is the whole reason the
  // instance is held across the bind.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1, { timeout: 20_000 });
  await expect(page.locator('.xterm[data-attached-before-bind]')).toHaveCount(1);
  await expect(page.locator('.xterm-rows')).toContainText('fake-claude session');
  await expect(page.getByText('This session is closed.')).toHaveCount(0);
});

test('a starting session with no pane yet is described as starting, not as closed', async ({
  page,
  server,
}) => {
  await server.restart({ DELTA_LAUNCH_DEADLINE_MS: LAUNCH_DEADLINE_MS });

  await page.goto('/');
  await startNewSession(page, 'starting-attach hold the hook back');
  await expect(
    page.getByRole('status', { name: 'Starting', exact: true }),
  ).toHaveCount(1, { timeout: 5_000 });
  await page.getByRole('button', { name: 'Terminal', exact: true }).click();

  // A browser with no pane to attach to. The real such window — between the
  // send being accepted and the launch finishing its preparation — is
  // milliseconds long on a session with no worktree to check out, far too short
  // to drive; a reload reproduces it exactly, because the announcement that the
  // pane came up is a live event and this browser was not there for it. Both are
  // the same state: the session is starting and this client knows of no pane.
  await page.reload();
  await expect(
    page.getByRole('status', { name: 'Starting', exact: true }),
  ).toHaveCount(1);

  // It says so, rather than telling the user to resume a session that was never
  // closed — the wording the terminal used to fall back to for anything that was
  // not open.
  await expect(page.getByText(/still starting up/i)).toBeVisible();
  await expect(page.getByText('This session is closed.')).toHaveCount(0);

  // And it is a window, not a dead end: the launch binds and the terminal
  // attaches to the session it is focused on.
  await expect(
    page.getByRole('status', { name: 'Open', exact: true }),
  ).toHaveCount(1, { timeout: 20_000 });
  await expect(page.locator('.xterm-rows')).toContainText('fake-claude session');
});
