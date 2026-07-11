import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * Cancelling Claude Code's `AskUserQuestion` from the UI — through the real
 * loop, including the Escape keystroke reaching the pane.
 *
 * Scenario `ask-user-question-cancel`: the fake calls `AskUserQuestion` and
 * fires `PermissionRequest`, exactly like `ask-user-question`. But instead of
 * scripting the answer's `tool_result` after a delay, the fake BLOCKS on an
 * `await_escape` step until a real Escape byte arrives in its pane — only then
 * does it write the `is_error` `tool_result` that cancels the call. So unlike
 * the answer spec (where the mock TUI cannot observe the injected keystrokes),
 * this spec genuinely proves the Cancel button's Escape injection reaches the
 * pane: the card cannot clear unless the Escape lands.
 *
 * What this validates: clicking Cancel POSTs to the cancel endpoint, the server
 * injects `Escape` into the pane, the fake unblocks and flushes the `is_error`
 * `tool_result`, and that resolution clears the card through the same
 * `permission_resolved` path an answer clears on.
 */
test('Cancel injects Escape into the pane and the is_error result clears the card', async ({
  page,
}) => {
  // Count the cancel POSTs so the test can assert the Cancel button reaches the
  // endpoint (the real server then injects Escape into the pane).
  let cancelPosts = 0;
  await page.route('**/api/sessions/*/questions/cancel', async (route) => {
    cancelPosts += 1;
    await route.continue();
  });

  await page.goto('/');
  await startNewSession(page, 'ask-user-question-cancel please pick a framework');

  // The dedicated question card appears with the question and its options.
  const card = page.getByTestId('question-card');
  await expect(card).toBeVisible();
  await expect(card).toContainText('Framework');
  await expect(card).toContainText('Which framework should we use?');

  // The card is still up (the fake is blocked on await_escape): clicking Cancel
  // POSTs to the cancel endpoint, which injects Escape into the pane.
  await card.getByTestId('question-cancel').click();
  await expect.poll(() => cancelPosts).toBeGreaterThan(0);

  // The authoritative clear is the resolution path: the Escape unblocks the
  // fake, which flushes the scripted `is_error` `tool_result`, resolving the
  // question's request row and clearing the card — exactly as an answer would.
  // The card can ONLY clear if the Escape actually reached the pane.
  await expect(card).toHaveCount(0);
  await expect(page.getByText('no problem, cancelled')).toBeVisible();
  await expect(page.getByTestId('permission-notice')).toHaveCount(0);
});
