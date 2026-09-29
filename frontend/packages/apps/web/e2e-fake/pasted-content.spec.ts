import fs from 'node:fs';
import path from 'node:path';
import { test, expect, type Page } from './support/fixtures';
import {
  sendMessage,
  shownTerminal,
  startNewSession,
} from './support/app';
import { fetchSends, latestSession } from './support/rest';

/**
 * Claude Code's `<pasted_content>` wrapper, end to end.
 *
 * Recent Claude Code builds wrap a paste of 20 or more characters in a
 * `<pasted_content id="xxxx">` block, in both the `UserPromptSubmit` hook's
 * `prompt` and the transcript's user line. Delta delivers every send as a
 * bracketed paste, so every long enough send comes back wrapped. Both
 * scenarios here turn the fake's `wrap_pastes` on, so the fake echoes a send
 * exactly that way, and each spec also reads the fake's transcript to prove
 * the wrapped form really was submitted — a silently bare echo would
 * otherwise pass for the tagged path.
 *
 * - Scenario `pasted-content-echo`: a plain follow-up send must still be
 *   matched to its tagged echo — rendered once, as the text that was sent,
 *   with no external-input notice raised for it.
 * - Scenario `pasted-content-branch` (the `branch-defer` script with the
 *   wrapper on): a branch sent from a quoted passage must still get its quote
 *   frame when its echo is tagged. The fake's reply echoes the
 *   `additionalContext` it received, so the quoted passage appearing in it is
 *   the quote frame arriving.
 */

/** The opening tag, with any id. */
const OPEN_TAG = /<pasted_content id="[0-9a-f]{4}">/;

/**
 * The prompt strings of every `type: "user"` line across the fake's
 * transcripts that contain `needle`.
 */
function userPromptsContaining(transcriptDir: string, needle: string): string[] {
  const prompts: string[] = [];
  for (const file of fs.readdirSync(transcriptDir)) {
    if (!file.endsWith('.jsonl')) {
      continue;
    }
    const content = fs.readFileSync(path.join(transcriptDir, file), 'utf8');
    for (const line of content.split('\n')) {
      if (line.trim() === '') {
        continue;
      }
      const record = JSON.parse(line) as {
        type?: string;
        message?: { content?: unknown };
      };
      const text = record.message?.content;
      if (record.type === 'user' && typeof text === 'string' && text.includes(needle)) {
        prompts.push(text);
      }
    }
  }
  return prompts;
}

/**
 * The exact form Claude Code submits a prompt that is nothing but a paste of
 * `body` in: the block with one id on both tags, newline-delimited.
 */
function wrappedForm(body: string): RegExp {
  const escaped = body.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(
    `^\\n\\n<pasted_content id="([0-9a-f]{4})">\\n${escaped}\\n</pasted_content id="\\1">\\n$`,
  );
}

/** Record every frame the live `/ws` event socket delivers to the page. */
function recordLiveFrames(page: Page): string[] {
  const frames: string[] = [];
  page.on('websocket', (socket) => {
    if (new URL(socket.url()).pathname !== '/ws') {
      return;
    }
    socket.on('framereceived', ({ payload }) => {
      frames.push(typeof payload === 'string' ? payload : payload.toString('utf8'));
    });
  });
  return frames;
}

test('a long send is matched to its pasted-content echo and shown without the tags', async ({
  page,
  server,
}) => {
  const frames = recordLiveFrames(page);
  await page.goto('/');
  // The positional first prompt never goes through the pane, so it is bare.
  await startNewSession(page, 'pasted-content-echo open the session');
  const messages = page.getByTestId('message-item');
  await expect(messages).toHaveCount(2);

  const body = 'a follow-up long enough for Claude Code to wrap it';
  await sendMessage(page, body);
  await expect(messages).toHaveCount(4);

  // The fake really did submit the wrapped form (hook prompt and transcript
  // line carry the same string).
  await expect
    .poll(() => userPromptsContaining(server.transcriptDir, body))
    .toHaveLength(1);
  const [submitted] = userPromptsContaining(server.transcriptDir, body);
  expect(submitted).toMatch(wrappedForm(body));

  // Rendered once, as the text that was sent.
  await expect(messages.filter({ hasText: body })).toHaveCount(1);
  await expect(messages.nth(2)).toContainText(body);
  await expect(messages.nth(2)).not.toContainText('pasted_content');
  await expect(page.getByText(OPEN_TAG)).toHaveCount(0);

  // The echo consumed the send: nothing stays open and the turn is idle.
  const session = await latestSession(page);
  await expect
    .poll(async () => {
      const sends = await fetchSends(page, session.id);
      return { open: sends.sends.length, turn: sends.turn.state };
    })
    .toEqual({ open: 0, turn: 'idle' });

  // Matched, so it was never announced as input from outside Delta.
  expect(frames.some((frame) => frame.includes('"external_input"'))).toBe(false);
});

test('a branch whose echo is pasted-content-tagged still gets its quote frame', async ({
  page,
  server,
}) => {
  await page.goto('/');
  await startNewSession(page, 'pasted-content-branch and hold the turn open');

  // The quotable reply lands; the turn stays in flight until Escape.
  const messages = page.getByTestId('message-item');
  await expect(messages).toHaveCount(2);

  // Select the assistant passage to set the branch origin (see
  // `branch-defer.spec.ts`).
  await messages.nth(1).evaluate((article) => {
    const content = article.querySelector('[class*="space-y"]') ?? article;
    const range = document.createRange();
    range.selectNodeContents(content);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    content.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
  });
  await expect(page.getByText('Branch from selected text')).toBeVisible();

  // A branch body long enough to be wrapped. Sent mid-turn it is held
  // queued, then dispatched once the turn ends.
  const body = 'follow up on that quoted passage, please';
  await sendMessage(page, body);
  await expect(page.getByText('queued — sends when idle')).toBeVisible();

  // End the turn from the embedded terminal.
  // A new session's terminal is open by default on this (large) layout.
  const xtermInput = shownTerminal(page).locator('.xterm-helper-textarea');
  await expect(xtermInput).toBeAttached();
  await expect(shownTerminal(page).locator('.xterm-rows')).toContainText(
    'fake-claude session',
  );
  await expect(async () => {
    await xtermInput.focus();
    await xtermInput.press('Escape');
    await expect(page.getByText('queued — sends when idle')).toHaveCount(0, {
      timeout: 2_000,
    });
  }).toPass({ timeout: 20_000 });

  // The dispatched branch send was echoed wrapped.
  await expect
    .poll(() => userPromptsContaining(server.transcriptDir, body), { timeout: 15_000 })
    .toHaveLength(1);
  const [submitted] = userPromptsContaining(server.transcriptDir, body);
  expect(submitted).toMatch(wrappedForm(body));

  // The tagged echo still earned the locator quote: the fake's reply shows
  // the additionalContext it received, which carries the quoted passage.
  await expect(
    page.getByText(/context received:[\s\S]*a quotable passage to branch from/),
  ).toBeVisible({ timeout: 15_000 });
  // The branch thread shows the send as written, never the raw tag text.
  await expect(messages).toHaveCount(2);
  await expect(messages.nth(0)).toContainText(body);
  await expect(messages.nth(0)).not.toContainText('pasted_content');
  await expect(page.getByText(OPEN_TAG)).toHaveCount(0);
});
