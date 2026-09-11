import { test, expect, type Page } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * The streamed reply text — this spec's single point of coupling to the fake:
 * it must stay equal to both the joined `stream_text` deltas and the `reply`
 * text of `./scenarios/streaming.json`. Every assertion below is scoped to this
 * string, so a fixture edit that changes it makes the mid-turn poll time out on
 * a bubble it no longer recognises.
 */
const REPLY_TEXT = 'Streaming this reply live.';

/** One atomic look at where the streamed reply text currently shows. */
type ReplySighting = {
  /** The provisional live bubble is on screen carrying the reply text. */
  liveBubbleShowsReply: boolean;
  /** How many persisted transcript lines carry the reply text. */
  persistedWithReply: number;
};

/**
 * Read both halves of the handoff in ONE pass over the DOM.
 *
 * Two separate locator assertions would be two reads at two instants, and the
 * handoff can land in the gap between them — the very kind of race this spec
 * is not allowed to have. A single `evaluate` sees one DOM, so "the bubble
 * shows the reply" and "nothing persisted carries it" are statements about the
 * same moment.
 */
const readReplySighting = (page: Page): Promise<ReplySighting> =>
  page.evaluate((text) => {
    const carries = (el: Element) => (el.textContent ?? '').includes(text);
    const bubble = document.querySelector('[data-testid="streaming-message"]');
    return {
      liveBubbleShowsReply: bubble !== null && carries(bubble),
      persistedWithReply: Array.from(
        document.querySelectorAll('[data-testid="message-item"]'),
      ).filter(carries).length,
    };
  }, REPLY_TEXT);

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
 *    text, and that text is NOT yet carried by any persisted `message-item`.
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
 *
 * Every assertion is phrased over the REPLY TEXT, never over the total number
 * of transcript lines on screen. The fake's hold bounds when the reply is
 * *persisted*, but nothing bounds when the browser's transcript refetch lands
 * the lines written earlier in the turn, so any total count pinned mid-turn is
 * a race.
 */
test('the assistant reply streams live then hands off without duplicating in a tool turn', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'streaming please stream this');

  // The provisional live bubble appears with the streamed text while the turn
  // is still in flight (the fake holds it open before writing the transcript
  // line), and at that moment no persisted transcript line carries that text.
  // The poll yields `null` until the bubble is sighted, so a bubble that never
  // appears fails here (expected 0, received null) rather than passing blind.
  await expect
    .poll(
      async () => {
        const { liveBubbleShowsReply, persistedWithReply } =
          await readReplySighting(page);
        return liveBubbleShowsReply ? persistedWithReply : null;
      },
      { timeout: 15_000 },
    )
    .toBe(0);

  // The reply text is persisted, then a separate tool_use line is written, so
  // the streamed text's transcript line is NOT the last assistant message. The
  // content-based gate (which scans every assistant message) still drops the
  // provisional bubble — so the reply text is present exactly once, never
  // duplicated across the live bubble and the persisted copy, even mid-turn
  // before the tool_result and Stop land.
  const persistedReply = page
    .getByTestId('message-item')
    .filter({ hasText: REPLY_TEXT });
  await expect(persistedReply).toHaveCount(1, { timeout: 15_000 });
  await expect(page.getByTestId('streaming-message')).toHaveCount(0, {
    timeout: 15_000,
  });
  await expect(page.getByText(REPLY_TEXT, { exact: true })).toHaveCount(1);
});
