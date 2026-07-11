import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * A mid-turn branch send defers, dispatches when the turn ends, and its
 * locator quote is delivered — through the real loop.
 *
 * Scenario `branch-defer`: the fake answers the first prompt with a quotable
 * passage and then holds the turn open (no `Stop`) until Escape. While the
 * turn is in flight the user selects that passage and sends a branch
 * follow-up. Dispatching it mid-turn would push it into Claude's own queue,
 * where its `UserPromptSubmit` hook — and therefore its locator quote — would
 * be lost; instead the server holds it `queued` (visible on the chip). The
 * user then interrupts via the embedded terminal: the turn ends, the queued
 * send dispatches as an ordinary prompt, its `UserPromptSubmit` fires, and
 * the server's hook response injects the locator quote as
 * `additionalContext`. The fake echoes what it received into its reply, so
 * the assertion observes, end to end, exactly what context was delivered.
 */
test('a mid-turn branch send is queued, then dispatched with its quote on turn end', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'branch-defer and hold the turn open');

  // The quotable reply lands; the turn stays in flight (no Stop is scripted
  // until the interrupt), so the first send's chip stays up.
  const messages = page.getByTestId('message-item');
  await expect(messages).toHaveCount(2);

  // Select the assistant passage, as a user highlighting it would —
  // MessageItem reads window.getSelection() on mouseup to set the branch
  // origin.
  const passage = messages.nth(1);
  await passage.evaluate((article) => {
    const content = article.querySelector('[class*="space-y"]') ?? article;
    const range = document.createRange();
    range.selectNodeContents(content);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    content.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
  });
  await expect(page.getByText('Branch from selected text')).toBeVisible();

  // Send the branch follow-up mid-turn. The POST creates the branch child
  // thread and the pane drills into it; the server defers the send itself —
  // the chip shows the explicit queued state, and the branch transcript stays
  // empty (nothing was typed into the pane, so no user line ever matched).
  await page.getByRole('textbox').fill('follow up on that passage');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByText('queued — sends when idle')).toBeVisible();
  await expect(messages).toHaveCount(0);

  // End the turn from the embedded terminal (Escape → the fake writes the
  // interrupt marker, exactly like the real `claude`; no Stop fires). The
  // attach is asserted first so a bridge failure surfaces here, not as an
  // opaque Escape-retry timeout below.
  await page.getByRole('button', { name: 'Terminal', exact: true }).click();
  const xtermInput = page.locator('.xterm-helper-textarea');
  await expect(xtermInput).toBeAttached();
  await expect(page.locator('.xterm-rows')).toContainText('fake-claude session');

  // Land Escape in the fake's stdin; retried until its observable effect (the
  // queued chip leaving — the send was promoted and typed) lands.
  await expect(async () => {
    await xtermInput.focus();
    await xtermInput.press('Escape');
    await expect(page.getByText('queued — sends when idle')).toHaveCount(0, {
      timeout: 2_000,
    });
  }).toPass({ timeout: 20_000 });

  // The dispatched branch send's UserPromptSubmit carried the locator quote
  // as additionalContext, and the fake's scripted reply echoes it: the quoted
  // passage round-tripped server → hook response → model input. Both lines of
  // the branch turn land on the branch thread the pane is already in.
  await expect(
    page.getByText(/context received:[\s\S]*a quotable passage to branch from/),
  ).toBeVisible({ timeout: 15_000 });
  await expect(messages).toHaveCount(2);
});
