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

/**
 * Timeout for an assertion that a turn has *settled* — its optimistic pending
 * chip drained, or its message rendered. A send's chip is an optimistic local
 * twin that the client keeps up until the turn's `turn_completed` event lands
 * (see `localSends` in the live store): the send leaving the server's open list
 * (`matched`) and the assistant reply rendering both happen on the transcript
 * refetch, which can outrun the separate `Stop`→`turn_completed` broadcast that
 * actually drops the chip. On a loaded CI runner (2 vCPUs) that broadcast +
 * re-render lands several seconds after the reply is already on screen, so a
 * chip-drain assertion gated on the default 5 s timeout flakes while the
 * message-count assertion beside it passes. This is the same generous,
 * turn-completion-appropriate budget the rest of the fake suite already uses
 * for post-completion assertions (e.g. `queued-prompt`), not a tunable sleep.
 */
const TURN_SETTLE_MS = 15_000;

test('a reload mid-conversation restores the same threads and messages', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'reload-restore first message');

  // First turn completes: prompt + reply, pending drained. The chip drains on
  // the turn's completion broadcast, which can trail the rendered reply under
  // load — wait for it with the turn-settle budget, not the default 5 s.
  await expect(page.getByTestId('message-item')).toHaveCount(2);
  await expect(page.getByTestId('pending-item')).toHaveCount(0, {
    timeout: TURN_SETTLE_MS,
  });

  // Second turn through the normal open-session send path.
  await sendMessage(page, 'and a second message');
  await expect(page.getByTestId('message-item')).toHaveCount(4);
  await expect(page.getByTestId('pending-item')).toHaveCount(0, {
    timeout: TURN_SETTLE_MS,
  });

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
