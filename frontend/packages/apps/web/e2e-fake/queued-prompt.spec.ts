import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * A prompt queued mid-turn dequeues as a plain user turn — through the real
 * loop, in the format current claude records.
 *
 * Scenario `queued-prompt`: the fake answers the first prompt and holds the
 * turn open (no `Stop`). A prompt submitted while the turn is busy is
 * recorded only as a uuid-less `queue-operation` enqueue line — bookkeeping
 * Delta deliberately ignores, so nothing new surfaces yet. The user presses
 * Escape in the embedded terminal: the fake writes the interrupt marker
 * (no Stop, like the real `claude`), then dequeues the queued prompt —
 * replaying it as a plain user line that fires its own `UserPromptSubmit`,
 * exactly like a TUI-typed prompt — and answers it. The spec asserts the
 * whole sequence lands in the conversation: the dequeued prompt shows as a
 * user message (it matched no Delta send, so it is external input on the
 * main thread) followed by its reply, and nothing was dropped or
 * misattributed.
 */
test('a prompt queued mid-turn dequeues after the interrupt and lands with its reply', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'queued-prompt hold this turn until Escape');

  // The first reply lands; the turn stays in flight (no Stop is scripted
  // until after the interrupt), so the pending chip stays up. The enqueue
  // bookkeeping line is already in the transcript by now, and deliberately
  // surfaces nothing.
  const messages = page.getByTestId('message-item');
  const pending = page.getByTestId('pending-item');
  await expect(messages).toHaveCount(2);
  await expect(pending).toHaveCount(1);

  // Open the embedded terminal; assert the attach before pressing Escape so
  // a bridge failure surfaces here, not as an opaque retry timeout below.
  await page.getByRole('button', { name: 'Terminal', exact: true }).click();
  const xtermInput = page.locator('.xterm-helper-textarea');
  await expect(xtermInput).toBeAttached();
  await expect(page.locator('.xterm-rows')).toContainText('fake-claude session');

  // Land Escape in the fake's stdin; retried until its observable effect (the
  // chip draining on the ingested interrupt marker) lands. Extra Escapes are
  // harmless, exactly as in the real TUI.
  await expect(async () => {
    await xtermInput.focus();
    await xtermInput.press('Escape');
    await expect(pending).toHaveCount(0, { timeout: 2_000 });
  }).toPass({ timeout: 20_000 });

  // The interrupt freed the turn, so the queued prompt dequeued: it was
  // replayed as a plain user line (its own UserPromptSubmit fired) and the
  // fake answered it. The full conversation is user, reply, interrupt
  // marker, dequeued prompt, reply — five messages, all on the main thread
  // the pane is showing.
  await expect(messages).toHaveCount(5, { timeout: 15_000 });
  await expect(
    page.getByText('queued while the turn was busy'),
  ).toBeVisible();
  await expect(page.getByText('answering the queued prompt')).toBeVisible();
});
