import { test, expect } from '@playwright/test';
import { startNewSession } from './support/app';

/**
 * The assistant's reply streams into the conversation pane live, via the
 * `MessageDisplay` hook — before the transcript line is flushed — and then
 * hands off to the persisted message without a duplicate, including in a
 * TOOL-USING turn.
 *
 * Scenario `streaming`: the fake fires `MessageDisplay` chunks for the reply
 * (a fresh message_id, increasing index, the last `final`), holds the turn
 * open briefly so the provisional bubble is observable, writes the full
 * assistant TEXT transcript line, then — mirroring how the real `claude` splits
 * one assistant message into separate per-content-block transcript lines —
 * writes a SEPARATE assistant `tool_use` line (empty visible text) and holds
 * the turn open again before the tool_result and `Stop`. The spec asserts:
 *
 * 1. While the turn is in flight, a provisional live bubble shows the streamed
 *    text — and it is NOT a persisted `message-item` (no transcript line yet).
 * 2. Once the reply text is persisted, even though it is followed by a
 *    tool_use assistant line (so the streamed text is NOT the last assistant
 *    message), the provisional bubble is gone and the reply text is present
 *    exactly once — no duplicate at the handoff.
 *
 * The exactly-once assertion guards the tool-turn handoff-duplicate
 * regression: the provisional bubble is suppressed the instant ANY persisted
 * assistant message carries its text (a content-based gate that scans every
 * assistant message, not just the last), so the reply text can never appear
 * twice — even when a tool_use line trails the text line and the transcript
 * refetch lands before the turn-end event clears the buffer.
 */
test('the assistant reply streams live then hands off without duplicating in a tool turn', async ({
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

  // The reply text is persisted, then a separate tool_use line is written, so
  // the streamed text's transcript line is NOT the last assistant message. The
  // content-based gate (which scans every assistant message) still drops the
  // provisional bubble — so the reply text is present exactly once, never
  // duplicated across the live bubble and the persisted copy, even mid-turn
  // before the tool_result and Stop land.
  await expect(streaming).toHaveCount(0, { timeout: 15_000 });
  await expect(
    page.getByText('Streaming this reply live.', { exact: true }),
  ).toHaveCount(1);
});
