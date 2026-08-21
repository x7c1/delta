import { test, expect } from './support/fixtures';
import { startNewCodexSession, startNewSession } from './support/app';

/**
 * The session-scoped allow — through the real loop, on the Codex (adapter) path.
 *
 * A single Codex turn can raise a dozen approvals in a row, and answering each
 * one individually is the friction this decision exists to remove. The browser
 * offers "Allow for session" only where the provider's
 * `has_allow_for_session` capability says the value means something, and the
 * decision travels as its own wire value (`acceptForSession`) rather than being
 * flattened into a plain accept on the way down.
 *
 * That flattening is exactly what a screenshot cannot rule out: an allow and a
 * session-scoped allow look identical in the UI, and Delta cannot observe the
 * grant the provider then holds (the scope lives in the provider's session).
 * So the proof is the value itself — the fake echoes the decision string it
 * received back as an assistant message, and this spec reads that off the page.
 *
 * Scenario `codex-parallel-approvals` (shared by every Codex spec in the run):
 * three approvals outstanding at once, the turn parked until all are answered.
 * The fake does not implement the grant, so the remaining two are still asked;
 * that is fine here — this is about which value reached the provider.
 */

test('a session-scoped allow reaches the provider as its own decision', async ({
  page,
}) => {
  await page.goto('/');
  await startNewCodexSession(page, 'read three files at once');

  const notice = page.getByTestId('permission-notice');
  await expect(notice).toBeVisible();

  // The button is on screen because the focused session's provider declares the
  // capability — not because of anything the UI knows about "codex".
  const allowForSession = notice.getByTestId(
    'permission-notice-allow-for-session',
  );
  await expect(allowForSession).toBeVisible();

  await allowForSession.click();

  // The fake echoed back the decision string it was handed: the session-scoped
  // value survived the browser → server → adapter → provider path intact.
  await expect(page.getByText('acceptForSession')).toBeVisible();

  // The rest of the fan-out still answers normally, and the turn completes.
  await expect(notice).toBeVisible();
  await notice.getByRole('button', { name: 'Allow', exact: true }).click();
  await notice.getByRole('button', { name: 'Allow', exact: true }).click();
  await expect(notice).toHaveCount(0);
  await expect(page.getByText('all three files read')).toBeVisible();
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});

test('a Claude session is offered no session-scoped button', async ({
  page,
}) => {
  // The other side of the capability gate, on the provider whose permission hook
  // has no session-scoped form. Pinned in the browser (not only in the unit
  // test) because the flag crosses four hops to get here — the providers query,
  // the workspace's per-provider lookup, the transcript pane, the card — and a
  // break anywhere along that chain would put a button on screen that answers
  // `400` when pressed.
  await page.goto('/');
  // The Claude fake picks its scenario from the first word of the first prompt.
  await startNewSession(page, 'permission-decide then ask before the tool');

  const notice = page.getByTestId('permission-notice');
  await expect(notice).toBeVisible();
  await expect(
    notice.getByTestId('permission-notice-allow-for-session'),
  ).toHaveCount(0);

  // The decisions Claude does have are unaffected.
  await notice.getByRole('button', { name: 'Allow' }).click();
  await expect(notice).toHaveCount(0);
  await expect(page.getByText('tool ran after allow')).toBeVisible();
});
