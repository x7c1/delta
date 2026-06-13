import { test, expect } from '@playwright/test';
// The app-driving helpers are shared with the fake-mode suite: both lanes
// drive the same real frontend against a real backend; only the model behind
// the pane differs.
import { startNewSession } from '../e2e-fake/support/app';

/**
 * Full-loop smoke against the real `claude` CLI.
 *
 * One real turn: a prompt sent from the browser is dispatched into a real
 * tmux pane running the real `claude`, whose reply flows back through the
 * JSONL transcript and the HTTP hooks into the browser. This is the
 * full-stack canary the fake-mode suite re-enacts deterministically.
 *
 * The assertions are structural only — a user message and an assistant
 * message render, the pending indicator drains when `Stop` lands, and the
 * conversation survives a reload — never about the reply's wording, since
 * the model is non-deterministic. Quota: one minimal turn ("reply with one
 * word") per attempt.
 */
test('a browser prompt round-trips through the real claude and survives reload', async ({
  page,
}) => {
  const prompt = 'Reply with only the word: ok';

  // The session's working directory, provided by scripts/e2e-real.sh: a
  // fresh directory inside the repository, so the real claude — which raises
  // a first-run trust prompt in directories it has never been trusted in —
  // starts under the repository's already-established trust.
  const workdir = process.env.E2E_REAL_WORKDIR;
  if (!workdir) {
    throw new Error('E2E_REAL_WORKDIR is not set; run via scripts/e2e-real.sh');
  }

  await page.goto('/');
  await startNewSession(page, prompt, workdir);

  // Optimistically pending immediately after Send; the real spawn registers
  // (SessionStart/UserPromptSubmit hooks bind it) and focus switches to the
  // real session.
  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1);
  await expect(page.getByTestId('new-session-empty')).toHaveCount(0, {
    timeout: 60_000,
  });

  // The user's message is ingested from the real transcript, then the real
  // reply lands and the turn completes (Stop drains the pending chip).
  const items = page.getByTestId('message-item');
  await expect(items.filter({ hasText: prompt })).toHaveCount(1, {
    timeout: 60_000,
  });
  await expect(items).toHaveCount(2, { timeout: 60_000 });
  await expect(pending).toHaveCount(0, { timeout: 30_000 });

  // Persistent state, not browser memory, is the source of truth: the same
  // conversation renders after a reload.
  await page.reload();
  await expect(page.getByTestId('message-item')).toHaveCount(2, {
    timeout: 30_000,
  });
  await expect(page.getByText(prompt)).toBeVisible();
});
