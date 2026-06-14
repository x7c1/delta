import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * `AskUserQuestion` with a multi-question call whose second question is
 * multi-select — through the real loop.
 *
 * Scenario `ask-user-question-multi`: the fake calls `AskUserQuestion` with two
 * questions, Q1 single-select and Q2 multi-select, then fires
 * `PermissionRequest`, exactly like the real `claude`. This is the shape that
 * previously made the backend refuse the answer (a 400) and left Submit a silent
 * no-op; the generator now supports it.
 *
 * What this validates: the card renders both questions inline with interactive
 * options, collecting a per-question choice (a single-select click + a
 * multi-select toggle) and submitting POSTs to the answer endpoint, and the card
 * clears when the scripted `tool_result` resolves the question's request row.
 *
 * What it deliberately does NOT validate: the actual keystroke → selection in
 * the TUI (the fake is scripted and has no real TUI). The pinned key sequences
 * are validated by the backend's key-sequence generator + injection unit tests.
 */
test('a multi-question call with a multi-select renders, answers, and clears', async ({
  page,
}) => {
  let answerPosts = 0;
  await page.route('**/api/sessions/*/questions/*/answer', async (route) => {
    answerPosts += 1;
    await route.continue();
  });

  await page.goto('/');
  await startNewSession(page, 'ask-user-question-multi please pick a stack');

  const card = page.getByTestId('question-card');
  await expect(card).toBeVisible();
  // Both questions render with their headers and options.
  await expect(card).toContainText('Framework');
  await expect(card).toContainText('Languages');
  await expect(card).toContainText('React');
  await expect(card).toContainText('Rust');
  await expect(card).toContainText('Python');

  // A multi-question call collects a choice per question behind one Submit; the
  // Submit is disabled until every question has a selection.
  const submit = card.getByTestId('question-submit');
  await expect(submit).toBeDisabled();

  // Q1 single-select: pick React. Q2 multi-select: toggle Rust and Python.
  await card.getByTestId('question-option-0-0').click();
  await card.getByTestId('question-option-1-0').click();
  await card.getByTestId('question-option-1-2').click();
  await expect(submit).toBeEnabled();

  await submit.click();
  await expect.poll(() => answerPosts).toBeGreaterThan(0);

  // The authoritative clear is the resolution path: the scripted tool_result
  // resolves the question's request row and clears the card.
  await expect(card).toHaveCount(0);
  await expect(
    page.getByText('thanks, using React with Rust and Python'),
  ).toBeVisible();
});
