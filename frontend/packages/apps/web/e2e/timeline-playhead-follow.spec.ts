import { test, expect, type Locator } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The thread-timeline playhead must keep following an external thread
 * selection even after a cross-lane jump whose target never rendered.
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
 * stranded on the branch lane. This spec drives exactly that sequence and
 * asserts the playhead moves back onto the newly selected thread's lane.
 *
 * The thread is re-selected via the transcript breadcrumb's "main" crumb.
 * That routes through the same external `activeThreadId` change the left-pane
 * session list produces, but WITHOUT the overlay's remount-on-null transient
 * (an out-of-scope, separately-tracked issue that re-anchors the playhead to
 * the global tail and would otherwise make the assertion non-deterministic).
 * The left-pane session-list interleaving itself is covered by the manual,
 * on-hardware acceptance criterion.
 */

/** The horizontal centre of a locator's bounding box, in viewport px. */
async function centerX(locator: Locator): Promise<number> {
  const box = await locator.boundingBox();
  if (!box) {
    throw new Error('locator has no bounding box');
  }
  return box.x + box.width / 2;
}

test('the playhead follows a thread selection after a renders-nothing cross-lane jump times out', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // Expand the timeline (it starts collapsed).
  await page.getByTestId('thread-timeline-toggle').click();
  const dot = (uuid: string) =>
    page.locator(
      `[data-testid="thread-timeline-dot"][data-message-uuid="${uuid}"]`,
    );
  // The branch lane's renders-nothing carrier (uuid-b4, the tool_result) must
  // have a standalone mark.
  await expect(dot('uuid-b4')).toBeVisible();
  const carrierX = await centerX(dot('uuid-b4'));

  // Click the branch lane's carrier mark: a cross-lane jump to the branch
  // thread whose target (uuid-b4) never renders, so the DOM-ready poll runs to
  // SCROLL_DOM_READY_TIMEOUT_MS. Click via mouse coordinates on the dot's
  // centre — the axis line span sits above the dot and would intercept a
  // Locator click, but the axis click handler lives on the scroll container
  // and resolves the nearest mark from the pointer's clientX either way.
  const carrierBox = await dot('uuid-b4').boundingBox();
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

  // Select a different thread: the breadcrumb's "main" crumb returns to the
  // main thread — an external `activeThreadId` change the overlay must follow.
  await page
    .getByTestId('transcript-top-row')
    .getByRole('button', { name: 'main' })
    .click();

  // The playhead must move off the stranded branch carrier and onto the main
  // lane. Assert the main lane becomes the highlighted (active) lane and its
  // playhead segment is no longer sitting on the branch carrier's x. Before
  // the fix the latched counter kept the playhead on uuid-b4 (branch lane).
  const mainLane = page.locator(
    '[data-testid="thread-timeline-lane"][data-thread-id="1"]',
  );
  await expect(mainLane).toHaveAttribute('data-active', 'true', {
    timeout: 5000,
  });
  const mainLanePlayhead = mainLane.locator(
    '[data-testid="thread-timeline-playhead"]',
  );
  await expect
    .poll(async () => Math.abs((await centerX(mainLanePlayhead)) - carrierX), {
      timeout: 5000,
    })
    .toBeGreaterThan(20);
});
