import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * Interrupt drains the pending chip — through the real loop.
 *
 * Scenario `interrupt-hold`: the fake answers the prompt with an assistant
 * line but never fires `Stop`, so the turn stays in flight and the pending
 * chip stays up. The user presses Escape in the embedded terminal; the
 * keystroke travels browser → /pty bridge → tmux → the fake's stdin, which
 * writes the `[Request interrupted by user]` marker (and fires NO Stop hook,
 * like the real `claude`). The backend's transcript tail ingests the marker,
 * emits `turn_interrupted`, and the chip drains.
 */
test('pressing Escape in the embedded terminal drains the stuck pending chip', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'interrupt-hold this turn until Escape');

  // The optimistic pending chip appears, and stays while the fake holds the
  // turn open (its reply lands, but no Stop arrives).
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(page.getByTestId('message-item')).toHaveCount(2);
  await expect(pending).toHaveCount(1);

  // Open the embedded terminal. The attach itself is observable: once the
  // bridge's `tmux attach` client is connected, tmux redraws the pane into the
  // browser terminal, and the fake's banner ends with an identifying line that
  // sits on the cursor row — always in view however small the fitted viewport
  // is. Asserting it first makes an attach failure (e.g. the bridge client
  // dying at startup) fail HERE with a clear message, instead of below as an
  // opaque Escape-retry timeout.
  await page.getByRole('button', { name: 'Terminal', exact: true }).click();
  const xtermInput = page.locator('.xterm-helper-textarea');
  await expect(xtermInput).toBeAttached();
  await expect(page.locator('.xterm-rows')).toContainText('fake-claude session');

  // Land Escape in the fake's stdin. The redraw above proves the attach, but
  // input still crosses the WS bridge and tmux asynchronously, so the press is
  // retried until its observable effect (the chip draining) lands; extra
  // Escapes are harmless, exactly as in the real TUI.
  await expect(async () => {
    await xtermInput.focus();
    await xtermInput.press('Escape');
    await expect(pending).toHaveCount(0, { timeout: 2_000 });
  }).toPass({ timeout: 20_000 });

  // The interrupt marker line belongs to the aborted turn and is rendered as
  // part of the conversation; no fourth turn started.
  await expect(page.getByTestId('message-item')).toHaveCount(3);
});
