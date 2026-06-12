import { test, expect, type Page } from '@playwright/test';
import { sendMessage, startNewSession } from './support/app';
import {
  dropLiveSocket,
  interceptLiveSocket,
  restoreLiveSocket,
} from './support/liveSocket';
import { fetchMessageCount, fetchSends, latestSession } from './support/rest';

/**
 * Reconnect resync: after a dropped `/ws` socket the client reconnects on its
 * own and rebuilds its state from REST, so whatever the server did during the
 * outage — events are not replayed — must surface as if it had been live.
 *
 * Each spec forces a real disconnect (see `support/liveSocket.ts`), observes
 * the server's state over REST while the page is dark, and asserts that the
 * UI converges to that server truth after the automatic reconnection:
 *
 * - a turn that ran entirely during the outage appears, its chip drains, and
 *   nothing is duplicated;
 * - a send still open on the server keeps its chip across the reconnect (a
 *   regression guard: pending sends are a server resource, so a reconnect is
 *   a refetch, never a wipe) and the running indicator is re-seeded from the
 *   queryable turn state;
 * - a pending permission notice survives the reconnect, exactly once;
 * - a permission raised entirely DURING the outage appears after reconnect,
 *   re-seeded from the sends envelope's queryable `permission` field (the
 *   `permission_requested` broadcast was lost with the socket).
 */

/**
 * Wait for the navigator's connection indicator to report `open` again. The
 * generous timeout covers the client's reconnect backoff (500 ms growing to
 * seconds depending on how many attempts the blocked window consumed); the
 * expectation resolves as soon as the socket is up.
 */
async function expectReconnected(page: Page): Promise<void> {
  await expect(page.getByTestId('connection-indicator')).toHaveAttribute(
    'data-connection',
    'open',
    { timeout: 15_000 },
  );
}

test('a turn that completes during a socket outage is resynced on reconnect', async ({
  page,
}) => {
  await interceptLiveSocket(page);
  await page.goto('/');
  // Scenario `ws-reconnect`: an echo loop — every prompt gets a reply and a
  // completed turn.
  await startNewSession(page, 'ws-reconnect first message');

  const messages = page.getByTestId('message-item');
  const pending = page.getByTestId('pending-item');
  const indicator = page.getByTestId('connection-indicator');
  await expect(messages).toHaveCount(2);
  await expect(pending).toHaveCount(0);
  await expect(indicator).toHaveAttribute('data-connection', 'open');
  const session = await latestSession(page);

  // Drop the live socket; the indicator leaves `open` (closed, then cycling
  // through blocked reconnect attempts).
  await dropLiveSocket(page);
  await expect(indicator).not.toHaveAttribute('data-connection', 'open');

  // Send while dark. REST still works, so the server accepts the send and the
  // optimistic chip appears — but every event of the resulting turn is lost.
  await sendMessage(page, 'sent while disconnected');
  await expect(pending).toHaveCount(1);

  // Observe over REST (not the page) that the whole turn ran server-side:
  // both transcript lines landed, the open-send list drained, the turn ended.
  await expect(async () => {
    expect(await fetchMessageCount(page, session.mainThreadId)).toBeGreaterThanOrEqual(4);
    const sends = await fetchSends(page, session.id);
    expect(sends.sends).toHaveLength(0);
    expect(sends.turn.state).toBe('idle');
  }).toPass({ timeout: 10_000 });

  // The page is provably stale: it still shows the pre-outage transcript and
  // a chip for a send whose turn is already over.
  await expect(messages).toHaveCount(2);
  await expect(pending).toHaveCount(1);

  // Reconnect: the resync must converge the UI to the observed server truth —
  // the outage turn's two messages appear exactly once, the stale chip
  // drains, and nothing reads as running.
  await restoreLiveSocket(page);
  await expectReconnected(page);
  await expect(messages).toHaveCount(4);
  await expect(pending).toHaveCount(0);
  await expect(page.getByText('running', { exact: true })).toHaveCount(0);
});

test('a queued send keeps its chip and the running state is re-seeded across a reconnect', async ({
  page,
}) => {
  await interceptLiveSocket(page);
  await page.goto('/');
  // Scenario `ws-reconnect-busy`: the first turn replies, then holds open for
  // a scripted beat before Stop; the second prompt completes normally.
  await startNewSession(page, 'ws-reconnect-busy hold the turn open');

  const messages = page.getByTestId('message-item');
  const pending = page.getByTestId('pending-item');
  const running = page.getByText('running', { exact: true });
  await expect(messages).toHaveCount(2);
  await expect(running).toBeVisible();
  // The first send already matched its transcript line but its turn has not
  // ended, so its in-progress chip is up.
  await expect(pending).toHaveCount(1);
  const session = await latestSession(page);

  // Drop the socket mid-turn, then send a follow-up while dark. The session
  // is busy, so the server parks the send `queued` — confirmed over REST.
  await dropLiveSocket(page);
  await sendMessage(page, 'queued during the outage');
  await expect(async () => {
    expect((await fetchSends(page, session.id)).sends).toHaveLength(1);
  }).toPass({ timeout: 10_000 });

  await restoreLiveSocket(page);
  await expectReconnected(page);

  // The regression guard: the queued send is a server resource, so the
  // reconnect refetch must keep its chip (the old client-side FIFO was wiped
  // on reconnect and lost it). The turn is still in flight server-side, so
  // the running indicator must also be back — rebuilt from the queryable turn
  // state, since the `turn_started` event window is gone.
  await expect(
    pending.filter({ hasText: 'queued during the outage' }),
  ).toBeVisible();
  await expect(running).toBeVisible();

  // The scripted hold ends: the first turn stops, the queued send dispatches,
  // and its turn completes. The generous timeout covers the scenario's
  // deliberate 10 s hold.
  await expect(messages).toHaveCount(4, { timeout: 20_000 });
  await expect(pending).toHaveCount(0);
  await expect(running).toHaveCount(0);
});

test('a pending permission notice survives a reconnect, exactly once', async ({
  page,
}) => {
  await interceptLiveSocket(page);
  await page.goto('/');
  // Scenario `ws-reconnect-permission`: a tool call raises a real permission
  // dialog, holds for a scripted beat, then the tool result resolves it and
  // the turn completes.
  await startNewSession(page, 'ws-reconnect-permission ask before the tool');

  const notice = page.getByTestId('permission-notice');
  await expect(notice).toBeVisible();

  // Drop and reconnect while the dialog is still pending in the TUI.
  await dropLiveSocket(page);
  await expect(page.getByTestId('connection-indicator')).not.toHaveAttribute(
    'data-connection',
    'open',
  );
  await restoreLiveSocket(page);
  await expectReconnected(page);

  // The notice survived the reconnect — not cleared by the resync, and not
  // duplicated by it either.
  await expect(notice).toHaveCount(1);

  // The scripted tool_result lands: the notice resolves and the turn
  // completes normally (prompt, tool call, closing reply). The generous
  // timeout covers the scenario's deliberate hold plus the decision-deadline
  // pass-through.
  await expect(notice).toHaveCount(0, { timeout: 20_000 });
  await expect(page.getByTestId('message-item')).toHaveCount(3);
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});

test('a permission raised entirely during the outage appears after reconnect', async ({
  page,
}) => {
  await interceptLiveSocket(page);
  await page.goto('/');
  // Scenario `ws-reconnect-permission-outage`: the first turn completes
  // normally; the second prompt triggers a tool call whose permission dialog
  // holds for a scripted beat before the tool result resolves it.
  await startNewSession(page, 'ws-reconnect-permission-outage first turn');

  const messages = page.getByTestId('message-item');
  const notice = page.getByTestId('permission-notice');
  await expect(messages).toHaveCount(2);
  await expect(notice).toHaveCount(0);
  const session = await latestSession(page);

  // Drop the socket, then send the permission-raising prompt while dark: the
  // `permission_requested` broadcast is lost with the socket, so the notice
  // is unrecoverable from events alone.
  await dropLiveSocket(page);
  await expect(page.getByTestId('connection-indicator')).not.toHaveAttribute(
    'data-connection',
    'open',
  );
  await sendMessage(page, 'now raise a permission while dark');

  // Observe over REST that the server holds the pending dialog — the
  // queryable counterpart of the lost event. (The page itself may even show
  // the notice already: REST keeps working during the outage, so any sends
  // refetch — e.g. the one the send's own POST triggers — can seed it early.
  // The guarantee under test is convergence, not pre-reconnect staleness.)
  await expect(async () => {
    const sends = await fetchSends(page, session.id);
    expect(sends.permission).not.toBeNull();
    expect(sends.permission?.tool_name).toBe('Bash');
  }).toPass({ timeout: 10_000 });

  // Reconnect. The resync drops the event-reconstructed permission notices
  // (their resolution may have been missed) and re-seeds from the refetched
  // sends envelope — so the dialog raised while dark must be up, exactly
  // once, regardless of whether an outage-window refetch already showed it.
  await restoreLiveSocket(page);
  await expectReconnected(page);
  await expect(notice).toHaveCount(1);
  await expect(notice).toContainText('Permission requested: Bash');

  // The scripted tool_result lands: the notice resolves (the live
  // `permission_resolved` is back) and the second turn completes — the first
  // turn's two messages plus prompt, tool call, and closing reply. The
  // generous timeout covers the scenario's deliberate 10 s hold plus the
  // decision-deadline pass-through.
  await expect(notice).toHaveCount(0, { timeout: 20_000 });
  await expect(messages).toHaveCount(5);
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
});
