import { test, expect, type Page } from '@playwright/test';
import { MOCK_BIG_THREAD_KEY } from '@delta/api-mocks';
import { useManualEventControl } from './support/app';

/**
 * Regression guard for the "stick trap": scrolling UP slowly from the tail with
 * a mouse wheel, the view scrolls up a hair then snaps back to the bottom, over
 * and over — the user cannot escape the tail.
 *
 * The mechanism (distinct from the estimate-error yank guarded by
 * `transcript-scroll-jank.spec.ts`):
 *   1. At the bottom `stickRef` is true. The stick-recompute scroll listener
 *      sets `stickRef = distanceToBottom < STICK_THRESHOLD_PX (64)` on every
 *      scroll event.
 *   2. Kinetic wheel scrolling delivers ONE motion as many small (10–20px)
 *      scroll events, so a slow gesture stays inside the 64px zone across
 *      several events — stick stays true.
 *   3. Each small upward step mounts a new history row above the fold; measuring
 *      it changes `messagesTotalSize`.
 *   4. The re-pin `useLayoutEffect` keyed on `messagesTotalSize` sees
 *      `stickRef === true` and sets `scrollTop = scrollHeight`, snapping the
 *      user back to the bottom mid-gesture.
 *   5. Repeat — a slow scroll never escapes.
 *
 * The fix makes the user's gesture the highest-priority writer of scrollTop:
 * any USER upward movement (scrollTop decreased) unsticks immediately, even
 * inside the 64px zone, so the re-pin never fires against the gesture.
 *
 * The metric: from the very bottom, scroll up in MANY SMALL wheel steps and
 * sample the distance-to-bottom. A working scroll escapes the tail and stays
 * escaped; the trap keeps yanking the sample back under the 64px threshold, so
 * net progress collapses toward zero and the bottom is re-entered repeatedly.
 *
 * Scope of THIS (chromium mock-mode) spec: it is a full end-to-end regression
 * guard for the slow-kinetic-scroll-up flow — it asserts the scroll escapes the
 * tail and is never snapped back. It does NOT reproduce the trap at HEAD,
 * because the arming condition — a measurement-driven `messagesTotalSize` change
 * WHILE the position is still inside the 64px zone — does not occur under
 * chromium here: the tail-plus-overscan rows are all measured before the gesture
 * begins, and the first newly-mounted/re-measured row lands ~480px above the
 * tail, far outside the zone. The trap surfaces on WebKitGTK, where kinetic
 * delivery and re-measurement timing produce in-zone size changes mid-gesture.
 * The DETERMINISTIC reproduction of the mechanism (a re-pin snapping the tail
 * back after an in-zone upward scroll) therefore lives in the unit suite —
 * `TranscriptPane.test.tsx` › "direction-aware stick (the \"stick trap\")",
 * which fails on the pre-fix distance-only rule and passes with the fix.
 */

const MESSAGE_COUNT = 200;
// Small steps that keep the position inside the 64px stick zone across several
// events — the condition kinetic delivery creates on WebKitGTK.
const WHEEL_STEP_PX = 15;
const UP_STEPS = 60;
const STEP_WAIT_MS = 40;
const STICK_THRESHOLD_PX = 64;
const INTENDED_PROGRESS_PX = WHEEL_STEP_PX * UP_STEPS;

interface ScrollSample {
  top: number;
  distanceToBottom: number;
}

async function sampleScroll(page: Page): Promise<ScrollSample> {
  return page.evaluate(() => {
    let el = document.querySelector(
      '[data-testid="transcript-message-list"]',
    ) as HTMLElement | null;
    while (el && el !== document.body) {
      const oy = getComputedStyle(el).overflowY;
      if (oy === 'auto' || oy === 'scroll') break;
      el = el.parentElement;
    }
    if (!el) return { top: 0, distanceToBottom: 0 };
    return {
      top: el.scrollTop,
      distanceToBottom: el.scrollHeight - el.scrollTop - el.clientHeight,
    };
  });
}

test('slow kinetic scroll up from the tail escapes the bottom instead of snapping back', async ({
  page,
}) => {
  await page.addInitScript(
    ([key, count]) => {
      (window as unknown as Record<string, unknown>)[key as string] = count;
    },
    [MOCK_BIG_THREAD_KEY, MESSAGE_COUNT] as const,
  );
  await useManualEventControl(page);
  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto('/');

  await expect(page.getByTestId('message-item').first()).toBeVisible();
  // The app lands at the bottom of the thread; let the initial layout settle.
  await page.waitForTimeout(400);

  const list = page.getByTestId('transcript-message-list');
  const box = await list.boundingBox();
  if (box) {
    await page.mouse.move(box.x + box.width / 2, 400);
  }

  // Confirm we really start pinned at the bottom (inside the stick zone).
  const start = await sampleScroll(page);
  expect(start.distanceToBottom).toBeLessThan(STICK_THRESHOLD_PX);

  const samples: ScrollSample[] = [start];
  for (let i = 0; i < UP_STEPS; i += 1) {
    await page.mouse.wheel(0, -WHEEL_STEP_PX); // negative = scroll up
    await page.waitForTimeout(STEP_WAIT_MS);
    samples.push(await sampleScroll(page));
  }

  const finalDistance = samples[samples.length - 1].distanceToBottom;
  const maxDistance = Math.max(...samples.map((s) => s.distanceToBottom));

  // A "snap-back" is a sample that has returned to WITHIN the stick zone AFTER
  // the scroll had already escaped it (distance once exceeded a clear margin).
  // A healthy upward scroll escapes once and never re-enters the zone; the trap
  // re-enters it repeatedly as the re-pin yanks the view to the tail.
  let escaped = false;
  let snapBacks = 0;
  for (const s of samples) {
    if (s.distanceToBottom > STICK_THRESHOLD_PX * 3) {
      escaped = true;
    } else if (escaped && s.distanceToBottom < STICK_THRESHOLD_PX) {
      snapBacks += 1;
      escaped = false;
    }
  }

  console.log(
    `[stick-trap] finalDistance=${Math.round(finalDistance)} maxDistance=${Math.round(
      maxDistance,
    )} of ${INTENDED_PROGRESS_PX} intended, snapBacks=${snapBacks}`,
  );

  // The scroll must escape the tail and STAY escaped: end well clear of the
  // 64px stick zone, having reached most of the intended upward distance.
  expect(finalDistance).toBeGreaterThan(INTENDED_PROGRESS_PX * 0.7);
  // And it must not be repeatedly yanked back into the tail mid-gesture.
  expect(snapBacks).toBeLessThanOrEqual(1);
});
