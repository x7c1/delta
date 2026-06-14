import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * Claude Code's built-in `AskUserQuestion` tool — through the real loop.
 *
 * Scenario `ask-user-question`: the fake calls `AskUserQuestion` (firing
 * `PreToolUse` with its `{questions:[…]}` input) and then fires
 * `PermissionRequest`, exactly like the real `claude`. Delta drives a dedicated
 * interactive question card off the `PreToolUse` row and short-circuits the
 * `PermissionRequest` to an immediate passthrough — so the readable question +
 * its choices appear at once, with NO Allow/Deny and NO generic permission
 * notice.
 *
 * What this fake-mode suite validates: the card renders INLINE in the
 * conversation flow with interactive options, submitting an answer POSTs to the
 * answer endpoint, and the card clears when the scripted `tool_result` resolves
 * the question's request row (the same `permission_resolved` path a permission
 * notice clears on).
 *
 * What it deliberately does NOT validate: the actual keystroke → selection in
 * the TUI. The fake is scripted and has no real TUI, so it cannot receive the
 * injected keystrokes the way real `claude` does. The keystroke-injection loop
 * (the pinned key sequences) is validated by the manual real-claude probe and by
 * the backend's key-sequence generator + injection unit tests, not here.
 */
test('AskUserQuestion renders an inline interactive card and answering it clears it', async ({
  page,
}) => {
  // Count the answer POSTs so the test can assert submitting reaches the
  // endpoint (the mock backend has no real TUI, so it just accepts the answer).
  let answerPosts = 0;
  await page.route('**/api/sessions/*/questions/*/answer', async (route) => {
    answerPosts += 1;
    await route.continue();
  });

  await page.goto('/');
  await startNewSession(page, 'ask-user-question please pick a framework');

  // The dedicated question card appears with the question and its options.
  const card = page.getByTestId('question-card');
  await expect(card).toBeVisible();
  await expect(card).toContainText('Framework');
  await expect(card).toContainText('Which framework should we use?');
  await expect(card).toContainText('React');
  await expect(card).toContainText('Svelte');

  // An option carrying a `preview` renders it verbatim in a monospace block;
  // an option without one shows no preview block.
  await expect(card.getByTestId('question-option-preview-0-0')).toContainText(
    '<App />',
  );
  await expect(card.getByTestId('question-option-preview-0-1')).toHaveCount(0);

  // It is a question, not a gateable permission: no Allow/Deny, and the generic
  // permission notice must never appear for this tool.
  await expect(card.getByRole('button', { name: 'Allow' })).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Deny' })).toHaveCount(0);
  await expect(page.getByTestId('permission-notice')).toHaveCount(0);

  // The card is INLINE in the conversation flow (the scrolling transcript
  // body), not inside the bottom composer overlay: it lives inside the
  // scrolling body (the `overflow-y-auto` region that holds the messages),
  // whereas the floating composer overlay does not scroll. Asserting the card
  // is a descendant of the scroll body pins the inline placement.
  const inlineCard = page.locator(
    '.overflow-y-auto [data-testid="question-card"]',
  );
  await expect(inlineCard).toBeVisible();

  // Answering from the UI: clicking a single-select option submits immediately
  // and POSTs to the answer endpoint.
  await card.getByTestId('question-option-0-0').click();
  await expect.poll(() => answerPosts).toBeGreaterThan(0);

  // The authoritative clear is the resolution path: the fake flushes the
  // scripted tool_result, which resolves the question's request row and clears
  // the card — exactly as a terminal answer would.
  await expect(card).toHaveCount(0);
  await expect(page.getByText('thanks, using React')).toBeVisible();
  await expect(page.getByTestId('permission-notice')).toHaveCount(0);
});
