import { test, expect, type Page, type Locator } from '@playwright/test';
import { BRANCH_THREAD_ID, SESSION_ID } from '@delta/api-mocks';
import { emitEvent, useManualEventControl } from './support/app';

/**
 * The thread-timeline playhead must follow every external thread selection —
 * in particular selections made from the LEFT-PANE session list, the primary
 * user-facing path.
 *
 * Both specs route thread selection through the left pane: the session card
 * header re-selects the session's main thread, and the card's thread tree
 * selects a child thread. An earlier revision had to avoid this path and
 * select via the transcript breadcrumb instead, because every left-pane
 * selection triggers a messages refetch (WorkspaceScreen invalidates the
 * active thread's messages on bind) whose fresh `sortedMessages` array
 * identity made the overlay's index-preservation effect revert the
 * just-committed playhead reposition from a stale ref — deterministically in
 * mock mode. With the active message's UUID as the canonical playhead state
 * (the index is derived per render), an array-identity change can no longer
 * move the playhead, so the user-facing path is asserted directly.
 */

/** The horizontal centre of a locator's bounding box, in viewport px. */
async function centerX(locator: Locator): Promise<number> {
  const box = await locator.boundingBox();
  if (!box) {
    throw new Error('locator has no bounding box');
  }
  return box.x + box.width / 2;
}

/** A timeline mark by message uuid. */
function dot(page: Page, uuid: string): Locator {
  return page.locator(
    `[data-testid="thread-timeline-dot"][data-message-uuid="${uuid}"]`,
  );
}

/** A timeline lane row by thread id. */
function lane(page: Page, threadId: number): Locator {
  return page.locator(
    `[data-testid="thread-timeline-lane"][data-thread-id="${threadId}"]`,
  );
}

/** A rendered transcript message article by uuid (never a timeline dot). */
function article(page: Page, uuid: string): Locator {
  return page.locator(`article[data-message-uuid="${uuid}"]`);
}

/**
 * The transcript's scrolling body — the `scrollbar-none` Panel body that holds
 * the message articles (the navigator and timeline axis also use
 * `scrollbar-none`, so scope by a contained message-item).
 */
function paneBody(page: Page): Locator {
  return page.locator('.scrollbar-none', {
    has: page.locator('[data-testid="message-item"]'),
  });
}

/** The transcript body's live scroll metrics. */
async function paneMetrics(
  page: Page,
): Promise<{ scrollTop: number; distToBottom: number }> {
  return paneBody(page).evaluate((el) => ({
    scrollTop: el.scrollTop,
    distToBottom: el.scrollHeight - el.scrollTop - el.clientHeight,
  }));
}

/**
 * Vertical offset (px) of an article's top from the pane body's top. A
 * timeline jump lands the target near the pane top (`block: 'start'`, just
 * below the pinned top-region overlay), so a small positive offset means the
 * pane scrolled TO that article rather than leaving it at the tail (large
 * offset / off-screen) or unscrolled below an earlier turn.
 */
async function articleOffsetFromPaneTop(
  page: Page,
  uuid: string,
): Promise<number> {
  const body = await paneBody(page).boundingBox();
  const art = await article(page, uuid).boundingBox();
  if (!body || !art) {
    throw new Error('missing bounding box');
  }
  return art.y - body.y;
}

/** Click a timeline mark by its dot's centre (the axis handler resolves the
 *  nearest mark from the pointer's clientX). */
async function clickDot(page: Page, uuid: string): Promise<void> {
  const box = await dot(page, uuid).boundingBox();
  if (!box) {
    throw new Error(`dot ${uuid} has no bounding box`);
  }
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
}

/**
 * The focused session's card header button in the left-pane session list.
 * Clicking it re-selects the session's main thread (the main thread has no
 * row of its own in the thread tree). Identified by the card's repository
 * label, which is unique to the seeded session.
 */
function sessionCardHeader(page: Page): Locator {
  return page.getByTestId('session-node').filter({ hasText: 'dev/delta' });
}

/**
 * Assert that a lane's playhead sits on one of the given marks' x. The
 * playhead is a 1 px bar and each dot a few px wide, both centred on the same
 * axis x, so the centres must agree within a small tolerance.
 *
 * A LIST of uuids (the reposition target first, then the rest of the lane's
 * marks) rather than the single target: the external reposition lands on the
 * lane's latest large turn, but the pane → playhead follower may legitimately
 * refine the pick to another mark OF THE SAME LANE if its observation batch
 * slips past the reposition's suppression window on a slow machine. What the
 * fix guarantees — and what the pre-fix clobber deterministically violated —
 * is that the playhead ends up on the SELECTED thread's content, never
 * reverted to the previous lane's x.
 */
async function expectPlayheadOnLaneMarks(
  page: Page,
  threadId: number,
  uuids: string[],
): Promise<void> {
  const playhead = lane(page, threadId).locator(
    '[data-testid="thread-timeline-playhead"]',
  );
  const dotXs = await Promise.all(
    uuids.map((uuid) => centerX(dot(page, uuid))),
  );
  await expect
    .poll(
      async () => {
        const x = await centerX(playhead);
        return Math.min(...dotXs.map((dotX) => Math.abs(x - dotX)));
      },
      { timeout: 5000 },
    )
    .toBeLessThanOrEqual(3);
}

/**
 * Selecting a child thread — and then the main thread again — from the
 * left-pane session list must move the playhead and the active-lane highlight
 * onto the selected thread's lane, landing on that lane's latest large
 * (main-conversation) turn.
 *
 * This is the reproduced clobber scenario: before the UUID-canonical fix the
 * post-selection messages refetch reverted the committed reposition, leaving
 * the playhead stranded on the previous thread (deterministic in mock mode).
 */
test('the playhead follows child and main thread selections from the left-pane session list', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Expand the timeline (it starts collapsed).
  await page.getByTestId('thread-timeline-toggle').click();

  // Mount: the active thread is main, so the main lane carries the active
  // highlight. The playhead's exact mount x is NOT asserted here — the
  // pane → playhead follower immediately snaps it to the topmost visible
  // article on its first observation batch, and where that batch lands
  // depends on the pane's initial scroll settling (the mount anchor itself
  // is pinned by the component's unit tests). Waiting for the marks also
  // guarantees the timeline is fully populated before the selection below.
  await expect(lane(page, 1)).toHaveAttribute('data-active', 'true');
  await expect(dot(page, 'uuid-b3b')).toBeVisible();

  // Select the child thread from the left-pane thread tree (the "⤷" node,
  // distinct from the transcript's "Enter delta etymology" chip).
  await page.getByRole('button', { name: /⤷ delta etymology/ }).click();
  // The pane drilled into the branch thread…
  await expect(page.locator('[aria-current="page"]')).toHaveText(
    'delta etymology',
  );
  // …and the playhead + highlight land on the branch lane — the reposition
  // targets its latest large turn (uuid-b3b). Before the fix the
  // refetch-driven revert kept them on main.
  await expect(lane(page, 2)).toHaveAttribute('data-active', 'true', {
    timeout: 5000,
  });
  await expectPlayheadOnLaneMarks(page, 2, [
    'uuid-b3b',
    'uuid-b1',
    'uuid-b2',
    'uuid-b3',
    'uuid-b4',
  ]);

  // Select the main thread again via the session card header.
  await sessionCardHeader(page).click();
  await expect(lane(page, 1)).toHaveAttribute('data-active', 'true', {
    timeout: 5000,
  });
  await expectPlayheadOnLaneMarks(page, 1, [
    'uuid-a2',
    'uuid-u1',
    'uuid-a1',
    'uuid-u2',
  ]);
});

/**
 * The playhead must keep following thread selections even after a cross-lane
 * jump whose target never rendered.
 *
 * Precondition (see the `uuid-b3`/`uuid-b4` pair in the mock fixtures): the
 * branch lane carries a paired tool_use / tool_result. The tool_result message
 * (`uuid-b4`) is a `user` row, so it gets a mark on the timeline, but it
 * renders NOTHING in the transcript (the result is shown inline with its call).
 * Clicking that mark starts a cross-lane jump whose DOM-ready poll can never
 * resolve — there is no article for `uuid-b4` — so it runs to the timeout.
 *
 * Before the guard fix, that timeout latched the in-flight counter above zero
 * forever, and every later thread selection was silently swallowed: the
 * external-thread effect bailed on `counter > 0` and the playhead stayed
 * stranded on the branch lane. This spec drives exactly that sequence — with
 * the follow-up selection routed through the left-pane session list — and
 * asserts the playhead moves back onto the newly selected thread's lane.
 */
test('the playhead follows a left-pane thread selection after a renders-nothing cross-lane jump times out', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Expand the timeline (it starts collapsed).
  await page.getByTestId('thread-timeline-toggle').click();
  // The branch lane's renders-nothing carrier (uuid-b4, the tool_result) must
  // have a standalone mark.
  await expect(dot(page, 'uuid-b4')).toBeVisible();
  const carrierX = await centerX(dot(page, 'uuid-b4'));

  // Click the branch lane's carrier mark: a cross-lane jump to the branch
  // thread whose target (uuid-b4) never renders, so the DOM-ready poll runs to
  // SCROLL_DOM_READY_TIMEOUT_MS. Click via mouse coordinates on the dot's
  // centre — the axis line span sits above the dot and would intercept a
  // Locator click, but the axis click handler lives on the scroll container
  // and resolves the nearest mark from the pointer's clientX either way.
  const carrierBox = await dot(page, 'uuid-b4').boundingBox();
  if (!carrierBox) {
    throw new Error('carrier dot has no bounding box');
  }
  await page.mouse.click(
    carrierBox.x + carrierBox.width / 2,
    carrierBox.y + carrierBox.height / 2,
  );
  // The pane drilled into the branch thread — confirm via the breadcrumb.
  await expect(page.locator('[aria-current="page"]')).toHaveText(
    'delta etymology',
  );

  // Wait past the DOM-ready timeout (SCROLL_DOM_READY_TIMEOUT_MS = 1000 ms) so
  // the jump settles. Before the fix this is where the counter latched.
  await page.waitForTimeout(1300);

  // Select the main thread from the left-pane session list: the card header
  // re-selects the session's main thread — an external `activeThreadId`
  // change the overlay must follow.
  await sessionCardHeader(page).click();

  // The playhead must move off the stranded branch carrier and onto the main
  // lane. Assert the main lane becomes the highlighted (active) lane and its
  // playhead segment is no longer sitting on the branch carrier's x.
  await expect(lane(page, 1)).toHaveAttribute('data-active', 'true', {
    timeout: 5000,
  });
  const mainLanePlayhead = lane(page, 1).locator(
    '[data-testid="thread-timeline-playhead"]',
  );
  await expect
    .poll(async () => Math.abs((await centerX(mainLanePlayhead)) - carrierX), {
      timeout: 5000,
    })
    .toBeGreaterThan(20);
});

/**
 * A cross-lane axis click on another lane's LARGE dot must scroll the
 * transcript pane TO that message, and later streamed content must NOT yank the
 * pane away from it.
 *
 * This reproduces M2 deterministically: the branch lane's LAST large turn
 * (uuid-b3b) is a near-tail target, so the landing `scrollIntoView` clamps at
 * the container bottom. Pre-fix, that landing scroll re-armed the pane's
 * stick-to-bottom follow, so the very next streamed chunk glued the pane to the
 * new tail and pushed the jump target off-screen. The fix keeps stick disarmed
 * through the landing, so streamed content grows BELOW the fold and the pane
 * stays on the target.
 *
 * A short viewport forces the branch transcript to scroll so the yank (or its
 * absence) is measurable.
 */
test('a cross-lane jump to a near-tail turn stays put when later content streams in (no tail re-stick)', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.setViewportSize({ width: 1000, height: 520 });
  await page.goto('/');

  await page.getByTestId('thread-timeline-toggle').click();
  await expect(lane(page, 1)).toHaveAttribute('data-active', 'true');
  await expect(dot(page, 'uuid-b3b')).toBeVisible();

  // Click the branch lane's last large turn (uuid-b3b) from the main lane: a
  // cross-lane jump whose landing clamps at the bottom.
  await clickDot(page, 'uuid-b3b');

  // Drilled into the branch, playhead on the branch lane, target visible.
  await expect(page.locator('[aria-current="page"]')).toHaveText(
    'delta etymology',
  );
  await expect(lane(page, 2)).toHaveAttribute('data-active', 'true', {
    timeout: 5000,
  });
  await expect(article(page, 'uuid-b3b')).toBeVisible();
  // The playhead stays on the branch lane, on/near the clicked mark (the
  // pane → playhead follower may refine to another mark of the same lane).
  await expectPlayheadOnLaneMarks(page, 2, [
    'uuid-b3b',
    'uuid-b1',
    'uuid-b2',
    'uuid-b3',
    'uuid-b4',
  ]);

  // Stream a tall assistant chunk into the branch thread. Pre-fix (stick
  // re-armed by the bottom-clamped landing) the pane follows this to the new
  // tail; with the fix the pane stays put and the chunk grows below the fold.
  const tall = Array.from({ length: 40 }, (_, i) => `streamed line ${i}`).join(
    '\n',
  );
  await emitEvent(page, {
    kind: 'assistant_streaming',
    session_id: SESSION_ID,
    thread_id: BRANCH_THREAD_ID,
    message_id: 'stream-b',
    index: 0,
    final: false,
    delta: tall,
  });
  await expect(page.getByTestId('streaming-message')).toBeVisible();

  // The pane did NOT follow the stream to the tail: the freshly streamed
  // content sits well below the fold (pre-fix this collapsed to ≈ 0), and the
  // jump target uuid-b3b is still on screen rather than pushed off the top.
  await expect
    .poll(async () => (await paneMetrics(page)).distToBottom, { timeout: 5000 })
    .toBeGreaterThan(120);
  await expect(article(page, 'uuid-b3b')).toBeVisible();
  expect(await articleOffsetFromPaneTop(page, 'uuid-b3b')).toBeGreaterThan(0);
});

/**
 * A cross-lane axis click on another lane's renders-nothing SMALL mark (a
 * tool_result carrier with no transcript article) must leave the pane near the
 * target's timeline position — on the nearest rendering neighbor — after the
 * DOM-ready poll times out, NOT parked at the tail.
 *
 * uuid-b4 is the last message in the branch lane and renders nothing; its
 * nearest rendering neighbor is the preceding large turn uuid-b3b, so the
 * deterministic timeout fallback scrolls uuid-b3b into view (rather than
 * leaving the pane wherever the switch parked it).
 *
 * NOTE on this fixture: uuid-b4's only rendering neighbor (uuid-b3b) happens to
 * be the branch's tail turn, so the resulting scroll position is close to the
 * bottom either way — the deterministic difference between the fallback and the
 * pre-fix tail-park (fallback fires exactly once, releases the in-flight
 * counter, scrolls to the nearest rendering neighbor, stick stays disarmed) is
 * pinned by the unit tests. This spec exercises the timeout → fallback path
 * end-to-end and asserts the pane settles on the neighbor rather than crashing
 * or stranding the pane off the neighbor.
 */
test('an axis click on another lane\'s renders-nothing mark settles the pane on the nearest rendering neighbor after the DOM-ready timeout', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.setViewportSize({ width: 1000, height: 520 });
  await page.goto('/');

  await page.getByTestId('thread-timeline-toggle').click();
  await expect(dot(page, 'uuid-b4')).toBeVisible();

  // Click the branch lane's renders-nothing carrier (uuid-b4): its DOM-ready
  // poll can never land, so it runs to SCROLL_DOM_READY_TIMEOUT_MS (1000 ms).
  await clickDot(page, 'uuid-b4');
  await expect(page.locator('[aria-current="page"]')).toHaveText(
    'delta etymology',
  );

  // After the timeout the fallback scrolls the nearest rendering neighbor
  // (uuid-b3b) into view, anchored to the top region (block: 'start').
  await expect(article(page, 'uuid-b3b')).toBeVisible();
  await expect
    .poll(async () => articleOffsetFromPaneTop(page, 'uuid-b3b'), {
      timeout: 5000,
    })
    .toBeLessThan(220);
});

/**
 * Wheel-scrubbing the playhead rightward parks the target message at the
 * reading-region start line (message articles carry
 * `scroll-margin-top: var(--delta-top-region-reserve)`), leaving the PREVIOUS
 * turn partially visible in the reserve band just above it. A later re-render
 * (streaming, a background refetch) re-binds the pane→playhead
 * IntersectionObserver, whose initial-observation flush can land AFTER the
 * 200 ms programmatic-scroll guard expires. Before the reserve-line fix that
 * flush committed the raw topmost-visible article and yanked the playhead
 * LEFTWARD onto it (deterministic here: the playhead snapped from the scrubbed
 * target uuid-b3b back to the branch lane's first turn uuid-b1). The fix
 * selects the article that OWNS the reserve line, so the late flush resolves
 * back to the scrubbed target and the playhead stays put.
 *
 * A short viewport forces the transcript to scroll so the scrub actually parks
 * the target at the line.
 */
test('a wheel-scrubbed playhead stays on its mark when a re-render fires after the guard window (no snap-back-left)', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.setViewportSize({ width: 1000, height: 300 });
  await page.goto('/');

  await page.getByTestId('thread-timeline-toggle').click();
  await expect(lane(page, 1)).toHaveAttribute('data-active', 'true');
  await expect(dot(page, 'uuid-b3b')).toBeVisible();

  // Wheel-scrub rightward (newer) over the axis. Stepping through the global
  // large-turn list carries the playhead off the main lane's tail into the
  // branch lane and clamps on the branch's last large turn, uuid-b3b — the
  // scrubbed target. Notches are spaced past the wheel-step cooldown so each
  // commits.
  const anchor = await dot(page, 'uuid-a1').boundingBox();
  if (!anchor) {
    throw new Error('anchor dot has no bounding box');
  }
  await page.mouse.move(
    anchor.x + anchor.width / 2,
    anchor.y + anchor.height / 2,
  );
  for (let i = 0; i < 5; i += 1) {
    await page.mouse.wheel(0, 120);
    await page.waitForTimeout(160);
  }

  // The scrub drilled into the branch lane and landed the playhead on uuid-b3b.
  await expect(lane(page, 2)).toHaveAttribute('data-active', 'true', {
    timeout: 5000,
  });
  const branchPlayhead = lane(page, 2).locator(
    '[data-testid="thread-timeline-playhead"]',
  );
  const targetX = await centerX(dot(page, 'uuid-b3b'));
  // Where a leftward yank lands: pre-fix the escaped flush commits the
  // topmost-visible article, the branch lane's first turn uuid-b1.
  const yankX = await centerX(dot(page, 'uuid-b1'));
  await expect
    .poll(async () => Math.abs((await centerX(branchPlayhead)) - targetX), {
      timeout: 5000,
    })
    .toBeLessThanOrEqual(3);
  // Sanity: the scrubbed mark and the leftward-yank mark are far enough apart
  // that the post-rerender assertion below is meaningful.
  expect(targetX - yankX).toBeGreaterThan(8);

  // Trigger a re-render + observer re-bind by streaming content, then idle
  // well past the programmatic-scroll guard window so any escaped flush has
  // fired.
  await emitEvent(page, {
    kind: 'assistant_streaming',
    session_id: SESSION_ID,
    thread_id: BRANCH_THREAD_ID,
    message_id: 'stream-guard',
    index: 0,
    final: false,
    delta: 'streamed while idle',
  });
  await page.waitForTimeout(700);

  // The playhead is STILL on uuid-b3b — not yanked left. Pre-fix this
  // deterministically reverted onto the topmost-visible earlier turn.
  const settledX = await centerX(branchPlayhead);
  expect(Math.abs(settledX - targetX)).toBeLessThanOrEqual(3);
});
