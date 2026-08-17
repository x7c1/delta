import { test, expect, type Page, type Locator } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The workspace shell — the one flex row holding the navigator, the transcript
 * and the right pane — must never scroll. There is no user affordance to scroll
 * it, so any scroll that lands on it shifts the entire app half off-screen with
 * no scrollbar to bring it back.
 *
 * That is not hypothetical: with the shell as an `overflow-hidden` box it is
 * still a scroll container, and two independent ingredients combined into a
 * whole-app shift observed in dogfooding — (a) a pane leaked absolutely
 * positioned descendants past its own scroll box (the comms log's `sr-only`
 * spans, anchored outside the static scroller), giving the shell thousands of
 * px of invisible scrollable overflow, and (b) a timeline jump's
 * `scrollIntoView` walks ALL ancestor scroll containers, so whatever alignment
 * the transcript pane could not satisfy (a near-tail target clamps at the
 * pane's bottom) was pushed into the shell. On an affected session that made
 * every thread switch shift the whole viewport, deterministically.
 *
 * These tests pin the shell-side half of the fix (`overflow-clip`: not a
 * scroll container at all). They INJECT phantom overflow rather than
 * reproducing the comms leak, so they keep guarding the shell against the next
 * leaking descendant, whatever it is — the comms half has its own regression
 * in comms-pane.spec.ts.
 */

function dot(page: Page, uuid: string): Locator {
  return page.locator(
    `[data-testid="thread-timeline-dot"][data-message-uuid="${uuid}"]`,
  );
}

async function clickDot(page: Page, uuid: string): Promise<void> {
  const box = await dot(page, uuid).boundingBox();
  if (!box) {
    throw new Error(`dot ${uuid} has no bounding box`);
  }
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
}

/**
 * Give the shell scrollable overflow the way a leaking pane would: an
 * absolutely positioned descendant extending far below the viewport.
 */
async function injectPhantomOverflow(page: Page): Promise<void> {
  await page.getByTestId('workspace-shell').evaluate((shell) => {
    const leak = document.createElement('div');
    leak.style.position = 'absolute';
    leak.style.top = '0';
    leak.style.left = '0';
    leak.style.width = '1px';
    leak.style.height = '5000px';
    shell.appendChild(leak);
  });
}

async function shellScrollTop(page: Page): Promise<number> {
  return page
    .getByTestId('workspace-shell')
    .evaluate((shell) => shell.scrollTop);
}

test('the shell cannot be scrolled even when a descendant leaks overflow', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');
  await expect(page.getByTestId('workspace-shell')).toBeVisible();

  await injectPhantomOverflow(page);
  // Programmatic scroll is the superset: scrollIntoView, focus() reveals and
  // anchor navigation all reduce to it. On an `overflow-hidden` shell this
  // write sticks; `overflow-clip` makes it a no-op.
  await page
    .getByTestId('workspace-shell')
    .evaluate((shell) => {
      shell.scrollTop = 500;
    });
  expect(await shellScrollTop(page)).toBe(0);
});

test('a timeline jump’s scrollIntoView never shifts the shell', async ({
  page,
}) => {
  await useManualEventControl(page);
  // Short viewport: the branch transcript must scroll, so a near-tail landing
  // clamps at the pane bottom and leaves unsatisfied alignment for
  // scrollIntoView to push into ancestor scroll containers — the shape of the
  // dogfooding incident.
  await page.setViewportSize({ width: 1000, height: 520 });
  await page.goto('/');

  await page.getByTestId('thread-timeline-toggle').click();
  await expect(dot(page, 'uuid-b3b')).toBeVisible();

  // The full user flow first: a cross-lane jump with phantom overflow present.
  await injectPhantomOverflow(page);
  await clickDot(page, 'uuid-b3b');
  await expect(
    page.locator('article[data-message-uuid="uuid-b3b"]'),
  ).toBeVisible({ timeout: 5000 });
  // Cover the jump's one-frame scrollIntoView re-call as well.
  await page.waitForTimeout(200);
  expect(await shellScrollTop(page)).toBe(0);

  // Then the incident's worst case, which the scripted jump above cannot pin
  // by itself (its clamp leftover happens to sit inside the target's
  // scroll-margin, so even a scrollable shell absorbs nothing): the same
  // `scrollIntoView({ block: 'start' })` the jump path uses, on a target whose
  // alignment can only be satisfied by scrolling the shell itself. On an
  // `overflow-hidden` shell this walks up and shifts the whole app; a clipped
  // shell is not a scroll container, so the walk skips it.
  await page.getByTestId('workspace-shell').evaluate((shell) => {
    const marker = document.createElement('div');
    marker.style.position = 'absolute';
    marker.style.top = '3000px';
    marker.style.left = '0';
    marker.style.width = '1px';
    marker.style.height = '10px';
    shell.appendChild(marker);
    marker.scrollIntoView({ block: 'start' });
  });
  expect(await shellScrollTop(page)).toBe(0);
  expect(
    await page.evaluate(() => document.scrollingElement?.scrollTop ?? 0),
  ).toBe(0);
});
