import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * Claude Code's built-in `AskUserQuestion` tool — through the real loop.
 *
 * Scenario `ask-user-question`: the fake calls `AskUserQuestion` (firing
 * `PreToolUse` with its `{questions:[…]}` input) and then fires
 * `PermissionRequest`, exactly like the real `claude`. Delta drives a dedicated
 * question card off the `PreToolUse` row and short-circuits the
 * `PermissionRequest` to an immediate passthrough — so the readable question +
 * options appear at once, with NO Allow/Deny and NO generic permission notice.
 *
 * Answering happens in the terminal (out of scope here); the fake plays the
 * TUI-answered path after a delay (its `tool_result`), and the question card
 * clears once that result is ingested — the same `permission_resolved` path a
 * permission notice clears on.
 */
test('AskUserQuestion renders a question card, not a permission notice', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'ask-user-question please pick a framework');

  // The dedicated question card appears with the question and its options.
  const card = page.getByTestId('question-card');
  await expect(card).toBeVisible();
  await expect(card).toContainText('Framework');
  await expect(card).toContainText('Which framework should we use?');
  await expect(card).toContainText('React');
  await expect(card).toContainText('Svelte');

  // It is a question, not a gateable permission: no Allow/Deny, and the
  // generic permission notice must never appear for this tool.
  await expect(card.getByRole('button', { name: 'Allow' })).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Deny' })).toHaveCount(0);
  await expect(page.getByTestId('permission-notice')).toHaveCount(0);

  // The user answered in the TUI: the fake flushes the tool_result, which
  // resolves the question's request row and clears the card.
  await expect(card).toHaveCount(0);
  await expect(page.getByText('thanks, using React')).toBeVisible();
  await expect(page.getByTestId('permission-notice')).toHaveCount(0);
});
