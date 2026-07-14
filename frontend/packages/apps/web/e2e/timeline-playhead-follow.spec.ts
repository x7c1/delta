import { test, expect, type Page, type Locator } from '@playwright/test';
import { useManualEventControl } from './support/app';

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
