import { test, expect } from '@playwright/test';
import { sendMessage, startNewSession } from './support/app';

/**
 * A mid-conversation reload restores the same conversation.
 *
 * The transcript JSONL and the SQLite overlay are the source of truth; the
 * browser holds no state a reload may lose. After two completed turns
 * (scenario `reload-restore`: an echo loop), reloading must render the same
 * session with the same turns — both user messages present, in order, with
 * their replies.
 */
test('a reload mid-conversation restores the same threads and messages', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'reload-restore first message');

  // First turn completes: prompt + reply, pending drained.
  await expect(page.getByTestId('message-item')).toHaveCount(2);
  await expect(page.getByTestId('pending-item')).toHaveCount(0);

  // Second turn through the normal open-session send path.
  await sendMessage(page, 'and a second message');
  await expect(page.getByTestId('message-item')).toHaveCount(4);
  await expect(page.getByTestId('pending-item')).toHaveCount(0);

  await page.reload();

  // The same conversation renders from persistent state: same turn count,
  // both user-authored messages present and in send order.
  const items = page.getByTestId('message-item');
  await expect(items).toHaveCount(4);
  const first = page.getByText('reload-restore first message');
  const second = page.getByText('and a second message');
  await expect(first).toBeVisible();
  await expect(second).toBeVisible();
  // Ordering: the first prompt's item precedes the second's in the transcript.
  const texts = await items.allInnerTexts();
  const firstIndex = texts.findIndex((t) => t.includes('reload-restore first message'));
  const secondIndex = texts.findIndex((t) => t.includes('and a second message'));
  expect(firstIndex).toBeGreaterThanOrEqual(0);
  expect(secondIndex).toBeGreaterThan(firstIndex);
});
