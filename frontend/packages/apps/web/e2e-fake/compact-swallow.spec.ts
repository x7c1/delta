import { test, expect } from './support/fixtures';
import { startNewSession, sendMessage } from './support/app';

/**
 * Auto-`/compact` swallows the dispatched send's prompt — the server re-types
 * it and the chip drains, through the real loop, in the format current claude
 * records.
 *
 * Scenario `compact-swallow`: the fake answers the positional first prompt
 * (`reply` + `stop`) so the session reaches its first idle state. The
 * follow-up send goes through `send_line` and lands in the fake's
 * `swallow_prompt` step, which CONSUMES the typed prompt without firing
 * `UserPromptSubmit` — modelling Claude Code's TUI swallowing the keystroke
 * into the auto-`/compact` routine. The fake then writes the four-line
 * `/compact` group (caveat → command-name → summary → stdout), the last of
 * which is the `isCompactSummary:true` line. Ingesting that summary fires
 * `Effect::AutoCompactFinished`; the sync interactor calls
 * `redispatch_stuck_dispatched`, which re-types the stuck send through
 * `TmuxDriver::send_line`. The fake's next `await_prompt` consumes the
 * re-typed prompt, fires `UserPromptSubmit`, replies, and stops — at which
 * point the pending chip clears.
 *
 * Without the re-dispatch the chip would stay up forever (the regression):
 * `swallow_prompt` consumed the only keystroke and `compact_group` writes
 * no echo, so nothing else would ever drain the `Dispatched` row.
 */
test('auto-compact-swallowed send is re-typed and the pending chip clears', async ({
  page,
}) => {
  await page.goto('/');
  await startNewSession(page, 'compact-swallow opening prompt');

  // The positional first prompt is auto-submitted; the fake replies and
  // stops, so the session is idle before the dispatched-but-swallowed step.
  await expect(page.getByText('first turn ack')).toBeVisible({
    timeout: 15_000,
  });

  // Send a follow-up. The fake's next step is `swallow_prompt`, which reads
  // the typed prompt off stdin but fires no `UserPromptSubmit` and writes
  // nothing — modelling auto-`/compact` swallowing the prompt. The row
  // therefore stays `dispatched` behind a missing echo until the server
  // re-types it on observing the compact summary.
  await sendMessage(page, 'the actual prompt that got swallowed');

  const pending = page.getByTestId('pending-item');
  await expect(pending).toHaveCount(1, { timeout: 15_000 });

  // After the swallow, the fake writes the four-line `/compact` group. The
  // summary line (`isCompactSummary: true`) drives
  // `Effect::AutoCompactFinished`, which makes the server re-type the stuck
  // prompt. The fake then awaits the re-typed prompt and answers it; once the
  // echo lands the row is `matched` and the chip drains.
  await expect(page.getByText('second turn ack after compaction')).toBeVisible({
    timeout: 15_000,
  });
  await expect(pending).toHaveCount(0, { timeout: 15_000 });
});
