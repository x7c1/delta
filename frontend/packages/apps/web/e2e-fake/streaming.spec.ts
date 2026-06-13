import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * The assistant's reply streams into the conversation pane live, via the
 * `MessageDisplay` hook — before the transcript line is flushed — and then
 * hands off to the persisted message without a duplicate.
 *
 * Scenario `streaming`: the fake fires `MessageDisplay` chunks for the reply
 * (a fresh message_id, increasing index, the last `final`), holds the turn
 * open briefly so the provisional bubble is observable, then writes the full
 * assistant transcript line and fires `Stop`. The spec asserts:
 *
 * 1. While the turn is in flight, a provisional live bubble shows the streamed
 *    text — and it is NOT a persisted `message-item` (no transcript line yet).
 * 2. After the turn completes, the provisional bubble is gone and the reply is
 *    present exactly once as a persisted assistant message — no duplicate at
 *    the per-turn handoff.
 *
 * The exactly-once assertion guards the handoff-duplicate regression: the
 * provisional bubble is suppressed the instant the persisted copy of its text
 * exists (a content-based gate), so the reply text can never appear twice even
 * if the transcript refetch lands before the turn-end event clears the buffer.
 */
test('the assistant reply streams live then hands off to the persisted message', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'streaming please stream this');

  // The provisional live bubble appears with the streamed text while the turn
  // is still in flight (the fake holds it open before writing the transcript
  // line). At this point only the user turn is a persisted message-item — the
  // streamed reply is the provisional bubble, not yet a transcript line.
  const streaming = page.getByTestId('streaming-message');
  await expect(streaming).toContainText('Streaming this reply live.', {
    timeout: 15_000,
  });
  const messages = page.getByTestId('message-item');
  await expect(messages).toHaveCount(1);

  // The turn completes: the persisted reply renders via the normal pipeline as
  // a second message-item. The instant it lands, the content-based gate drops
  // the provisional bubble — so the reply text is present exactly once, never
  // duplicated across the live bubble and the persisted copy at the handoff.
  await expect(messages).toHaveCount(2, { timeout: 15_000 });
  await expect(streaming).toHaveCount(0, { timeout: 15_000 });
  await expect(
    page.getByText('Streaming this reply live.', { exact: true }),
  ).toHaveCount(1);
});
