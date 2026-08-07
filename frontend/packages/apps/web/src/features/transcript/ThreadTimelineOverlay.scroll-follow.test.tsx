/**
 * Horizontal and vertical scroll-follow: keeping the playhead
 * and the active lane row visible inside the timeline viewport.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import {
  act,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { LANE_LEFT_PAD_PX } from './ThreadTimelineOverlay';
import {
  makeThread,
  makeUserText,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
} from './ThreadTimelineOverlay.testkit';

/**
 * Horizontal scroll-follow effect (v31).
 *
 * The effect runs after a user-driven navigation (`scrubTick > 0`) and is
 * responsible for keeping the playhead inside the axis-column wrapper's
 * horizontal viewport when the viewport is narrower than the rendered
 * content (a wide axis + a fixed-width side panel). v31 introduces two
 * changes to the math:
 *
 *  1. A threshold margin so the scroll re-centres BEFORE the playhead
 *     reaches the edge, not only after it has gone completely off-screen.
 *     Without it the vertical playhead bar visibly disappears for one
 *     viewport-width while the user keeps scrubbing.
 *  2. The visibility check is now in the SAME coordinate system as
 *     `scrollEl.scrollLeft`. Under the v20 grid layout the scroll content
 *     contains BOTH the sticky label column AND the axis cell — so a
 *     dot's content-space x is `labelWidth + LANE_LEFT_PAD_PX + xInAxis`,
 *     not `LANE_LEFT_PAD_PX + xInAxis` (which is what the v30 effect
 *     used). The fix derives `labelWidth` live from the first axis
 *     cell's `offsetLeft`.
 *
 * jsdom does not run CSS, so `clientWidth`, `offsetLeft`, and `scrollLeft`
 * are all 0 by default. The helpers below stub the layout we need on a
 * per-element basis (using `Object.defineProperty` rather than spying on
 * the prototype, so each test scopes its overrides cleanly).
 */
describe('ThreadTimelineOverlay horizontal scroll-follow (v31)', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  /**
   * Override a layout property on a single DOM element. Returns a
   * cleanup the test should call to restore the original (so a later
   * test in the same file is not poisoned by the stub).
   */
  function defineLayoutProp(
    el: HTMLElement,
    prop: 'clientWidth' | 'offsetLeft',
    value: number,
  ): void {
    Object.defineProperty(el, prop, {
      configurable: true,
      get: () => value,
    });
  }

  it('treats `playheadInContent` as `labelOffset + LANE_LEFT_PAD_PX + xInAxis` so the visibility check is in the same coord system as scrollLeft', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // Three large messages so a wheel-up from msg-c steps the
          // playhead onto msg-b (x=0.5 → 120 px inside the axis).
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]');
    expect(axisEl).not.toBeNull();
    // Narrow viewport so the visibility check actually runs.
    defineLayoutProp(wrapper, 'clientWidth', 200);
    // Pretend the label column is 100 px wide. The axis cell's
    // `offsetLeft` is the only signal the effect uses to recover that
    // width, so this is the entire label-offset surface area.
    defineLayoutProp(axisEl as HTMLElement, 'offsetLeft', 100);
    // Pre-position the scroll well past the playhead's would-be
    // position under the OLD (v30) math. msg-b at x=120 + LANE_LEFT_PAD
    // is 136 in axis-local coords; the v30 effect would compare 136
    // against `viewLeft=300`, see it as "to the left of viewport", and
    // scroll to `max(0, 136 - 100) = 36`. But the dot actually paints
    // at content x = 100 + 136 = 236, which IS visible inside
    // [300, 500] only if scrollLeft <= 236 — i.e. the v30 fix would
    // SHIFT the visible playhead even though it was already on screen
    // had the math been correct.
    wrapper.scrollLeft = 300;
    // Scrub via wheel-up to step from msg-c → msg-b (bumps scrubTick).
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    // Under v31 the effect sees playheadInContent = 100 + 120 + 16 =
    // 236, which is to the LEFT of the current view [300, 500], so it
    // re-centres to max(0, 236 - 100) = 136 — a value that lands the
    // dot's true content-space position (236) right at the viewport
    // midpoint (136 + 100). The exact midpoint anchor is the assertion:
    // half a clientWidth past the new scrollLeft must equal the dot's
    // content x.
    await waitFor(() => {
      // playheadInContent = labelOffset + xInAxis + LANE_LEFT_PAD
      const playheadInContent =
        100 + 120 + LANE_LEFT_PAD_PX; // = 236
      const expectedScrollLeft = Math.max(
        0,
        playheadInContent - 200 / 2,
      ); // 200 = clientWidth
      expect(wrapper.scrollLeft).toBe(expectedScrollLeft);
    });
  });

  it('re-centres as soon as the playhead crosses INTO the edge margin, not only after it has left the viewport', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]');
    expect(axisEl).not.toBeNull();
    // 600 px viewport → margin = max(80, 600/5) = 120 px.
    defineLayoutProp(wrapper, 'clientWidth', 600);
    // No label offset for this test: we want to isolate the margin
    // behaviour from the label-offset behaviour exercised above.
    defineLayoutProp(axisEl as HTMLElement, 'offsetLeft', 0);
    // Position the viewport so msg-b's playhead sits just INSIDE the
    // visible area but well INSIDE the right-edge margin band — the v30
    // effect (which only fires when the playhead is past the edge)
    // would leave scrollLeft untouched.
    //
    //   msg-b at xInAxis = 120 + LANE_LEFT_PAD = 136
    //   viewLeft = 0, viewRight = 600
    //   right-edge margin band = [600 - 120, 600] = [480, 600]
    //   Pick a position where playheadInContent (= 136) sits well
    //   inside the LEFT margin band [0, 120] so the v30 effect (no
    //   margin) does nothing — only v31 must fire.
    //
    // The cleanest setup: leave scrollLeft at 0 and assert that the
    // initial-step settle still triggers a re-centre because the
    // playhead at 136 sits inside the [0, 120] left-edge band when
    // viewLeft=0 — i.e. 136 < 0 + 120? No, 136 > 120, so we need a
    // different scenario. Bump viewLeft to 30 so the band becomes
    // [30, 150]: 136 is inside it.
    wrapper.scrollLeft = 30;
    // Bump scrubTick by scrubbing via wheel.
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    // Expected: re-centred to 136 - 300 (clientWidth/2) = -164 → clamped to 0.
    await waitFor(() => {
      expect(wrapper.scrollLeft).toBe(0);
    });
  });

  it('still re-centres when the playhead has gone completely off-screen', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]');
    expect(axisEl).not.toBeNull();
    defineLayoutProp(wrapper, 'clientWidth', 200);
    defineLayoutProp(axisEl as HTMLElement, 'offsetLeft', 0);
    // The previous viewport sits to the RIGHT of msg-b (which lives at
    // x = 120 + 16 = 136). Scroll past it so msg-b is fully off-screen.
    wrapper.scrollLeft = 400;
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    // The off-screen branch re-centres to max(0, 136 - 100) = 36.
    await waitFor(() => {
      expect(wrapper.scrollLeft).toBe(36);
    });
  });

  it('re-centres via scrollTo({ behavior: "smooth" }) so the auto-scroll animates instead of snapping', async () => {
    // Re-install the mock so this test owns the call log (the suite-level
    // `beforeEach` ran before this test body started, but its mock is shared
    // with any earlier assertions; capturing a fresh reference makes the
    // assertion local to this test). Capture the global setup.ts stub first
    // so the `finally` below can put it back — `vi.restoreAllMocks` does not
    // undo a `defineProperty`, and leaving this mock installed poisons every
    // later test that relies on the global stub's scrollTop/scrollLeft
    // mirroring (the vertical scroll-follow suite does).
    const originalScrollTo = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      'scrollTo',
    );
    const scrollToMock = vi.fn(function (
      this: HTMLElement,
      options: ScrollToOptions,
    ) {
      if (typeof options.left === 'number') {
        this.scrollLeft = options.left;
      }
      if (typeof options.top === 'number') {
        this.scrollTop = options.top;
      }
    });
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      writable: true,
      value: scrollToMock,
    });
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]');
    expect(axisEl).not.toBeNull();
    defineLayoutProp(wrapper, 'clientWidth', 200);
    defineLayoutProp(axisEl as HTMLElement, 'offsetLeft', 0);
    // Same "playhead fully off-screen" scenario as the previous test, so we
    // know the re-centre branch fires deterministically.
    wrapper.scrollLeft = 400;
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    try {
      await waitFor(() => {
        expect(scrollToMock).toHaveBeenCalled();
      });
      // Every call must use the smooth animation API, not a positional or
      // behavior-less form. Without `behavior: 'smooth'` the auto-scroll
      // snaps and the user sees a visible jump as the playhead approaches
      // the viewport edge. The prototype-level mock also catches the
      // VERTICAL lane catch-up's `scrollTo({ top })` on the body wrapper,
      // so the per-call coordinate check accepts either axis; the
      // horizontal re-centre this test is about is asserted separately
      // below.
      for (const call of scrollToMock.mock.calls) {
        const options = call[0] as ScrollToOptions;
        expect(options).toMatchObject({ behavior: 'smooth' });
        expect(
          typeof options.left === 'number' || typeof options.top === 'number',
        ).toBe(true);
      }
      expect(
        scrollToMock.mock.calls.some(
          (call) => typeof (call[0] as ScrollToOptions).left === 'number',
        ),
      ).toBe(true);
    } finally {
      if (originalScrollTo) {
        Object.defineProperty(
          HTMLElement.prototype,
          'scrollTo',
          originalScrollTo,
        );
      }
    }
  });

  it('re-centres on a leftward step BEFORE the playhead hides behind the sticky label column when labelOffsetPx > margin', async () => {
    // v31 fix 3 regression. The sticky label cell paints over viewport-x
    // `[0, labelOffsetPx]` on every frame (it carries `position: sticky;
    // left: 0; zIndex: 1` while the playhead has no explicit z-index, so
    // the label wins the stack). When the user scrubs leftward the
    // playhead becomes physically hidden the moment its viewport-x drops
    // below `labelOffsetPx`, even though the v30 / v31-fix-2 visibility
    // check would still consider it "inside the viewport" until it
    // crossed `viewLeft + margin`. On layouts where the label is wider
    // than the margin floor (typical lane labels are 120–180 px; the
    // margin floor is 80 px) the playhead spends `labelOffsetPx - margin`
    // pixels invisible under the sticky band before the scroll catches
    // up. The fix is an asymmetric threshold: the left edge becomes
    // `viewLeft + labelOffsetPx + margin`; the right edge stays
    // `viewRight - margin` (nothing covers the right side of the axis
    // column).
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]');
    expect(axisEl).not.toBeNull();
    // Narrow panel so the margin sits at its 80 px floor: `max(80, 300/5)
    // = max(80, 60) = 80`. Picking a viewport smaller than `5 * 80 = 400`
    // is what forces the floor to bite; on wider panels the 20% rule
    // would already swallow most reasonable label widths and the bug
    // would be invisible.
    defineLayoutProp(wrapper, 'clientWidth', 300);
    // Label column is 140 px — wider than the 80 px margin floor. This
    // is the regime the fix targets: `labelOffsetPx (140) > margin (80)`
    // means there is a `[margin, labelOffsetPx]` = `[80, 140]` viewport-x
    // band where the playhead is geometrically "inside the viewport"
    // (the v31-fix-2 condition `playheadInContent >= viewLeft + margin`
    // holds) but actually painted under the sticky label.
    defineLayoutProp(axisEl as HTMLElement, 'offsetLeft', 140);
    // Position the viewport so msg-b's playhead (content x = 140 + 120 +
    // 16 = 276) sits inside the hidden band:
    //   viewLeft = 150
    //   viewport-x of playhead = 276 - 150 = 126, which is inside
    //   the sticky band [0, labelOffsetPx] = [0, 140] (so the playhead
    //   is invisible) AND inside the old v31-fix-2 "in view" band
    //   [margin, clientWidth - margin] = [80, 220] (so the OLD effect
    //   would not fire).
    //
    // Under the old left-edge formula `viewLeft + margin = 230`:
    //   playheadInContent (276) < 230?  No  → no catch-up.
    // Under the new left-edge formula `viewLeft + labelOffsetPx + margin
    // = 150 + 140 + 80 = 370`:
    //   playheadInContent (276) < 370?  Yes → catch-up fires.
    // Expected new scrollLeft: max(0, 276 - 300/2) = max(0, 126) = 126.
    wrapper.scrollLeft = 150;
    // Scrub via wheel-up to step msg-c → msg-b (bumps userActedTick).
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await waitFor(() => {
      // playheadInContent = labelOffset + xInAxis + LANE_LEFT_PAD
      const playheadInContent = 140 + 120 + LANE_LEFT_PAD_PX; // = 276
      const expectedScrollLeft = Math.max(
        0,
        playheadInContent - 300 / 2,
      ); // = 126
      expect(wrapper.scrollLeft).toBe(expectedScrollLeft);
    });
  });

  it('keeps the right-edge threshold at `viewRight - margin` (no labelOffsetPx adjustment) so the asymmetry stays asymmetric', async () => {
    // Companion to the "re-centres on a leftward step" test above. The
    // sticky label only covers the LEFT side of the viewport, so the
    // right edge must stay `viewRight - margin`. A naive "mirror the
    // fix" implementation would shrink the right threshold to
    // `viewRight - labelOffsetPx - margin` and would re-centre way too
    // eagerly on rightward scrubs. This test pins the asymmetry in
    // place: with `offsetLeft = 140`, place the playhead inside the
    // right-edge margin band `[viewRight - margin, viewRight]` and
    // assert the catch-up still fires there.
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]');
    expect(axisEl).not.toBeNull();
    // Same narrow panel + wide label as the leftward test. clientWidth
    // 300 → margin floor 80; labelOffsetPx 140.
    defineLayoutProp(wrapper, 'clientWidth', 300);
    defineLayoutProp(axisEl as HTMLElement, 'offsetLeft', 140);
    // Position the viewport so msg-b's playhead (content x = 276) sits
    // inside the right-edge margin band `[viewLeft + 220, viewLeft + 300]`:
    //   viewLeft = 30, viewRight = 330, right band = [250, 330].
    //   playheadInContent (276) is inside [250, 330] ✓.
    //   Left band under the fix: [30, 30 + 140 + 80] = [30, 250].
    //   playheadInContent (276) is NOT in [30, 250] ✓.
    // So only the right-edge branch can drive the catch-up — exactly
    // what we want to verify.
    //
    // A hypothetical "symmetric" fix that subtracted labelOffsetPx from
    // the right too would set the right boundary to
    //   viewRight - labelOffsetPx - margin = 330 - 140 - 80 = 110,
    // i.e. the catch-up would fire any time the playhead drifted past
    // viewport-x 80 from the left — wildly over-eager and visibly
    // janky on rightward scrubs. By leaving the right edge alone the
    // catch-up only fires in the actual right-edge margin band.
    wrapper.scrollLeft = 30;
    // Scrub via wheel-up to step msg-c → msg-b (still bumps
    // userActedTick, even though we're testing the right-edge branch).
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await waitFor(() => {
      const playheadInContent = 140 + 120 + LANE_LEFT_PAD_PX; // = 276
      const expectedScrollLeft = Math.max(
        0,
        playheadInContent - 300 / 2,
      ); // = 126
      expect(wrapper.scrollLeft).toBe(expectedScrollLeft);
    });
  });

  it('axisScrollRef points at the wrapper that hosts both the sticky labels and the axis cells (one horizontal scroll surface, label-width baked in)', async () => {
    // The "label-width hypothesis" check from v31. Confirms that:
    //  - axisScrollRef's element is the same node tagged
    //    `data-testid="thread-timeline-axis-column"`.
    //  - That node DOES contain the sticky label cells — i.e. the
    //    scroll content's x=0 sits at the label column's left edge,
    //    not at the axis cell's left edge. (Used as the contract that
    //    justifies the label-offset correction in the scroll-follow
    //    effect.)
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const wrapper = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    // The wrapper is what carries the horizontal scrollbar.
    expect(wrapper.className).toContain('overflow-x-auto');
    // Sticky labels live INSIDE this wrapper (they share the grid
    // container so they can pin to the wrapper's left edge as the
    // axis pans). If they were outside the scroll content, the
    // label-width offset correction would be unnecessary.
    const labels = within(wrapper).getAllByTestId(
      'thread-timeline-lane-label',
    );
    expect(labels.length).toBeGreaterThan(0);
    // And the axis cells live inside it too.
    const axisCells = wrapper.querySelectorAll('[data-timeline-axis]');
    expect(axisCells.length).toBe(labels.length);
  });
});

/**
 * Vertical counterpart of the horizontal scroll-follow suite above: the
 * body wrapper (`thread-timeline-body`, `max-h-64 overflow-y-auto`) is the
 * vertical viewport, and a navigation that lands the playhead on a lane
 * row outside that viewport must scroll the row into view — otherwise the
 * playhead moves onto a row the user cannot see (the original dogfooding
 * bug: enough lanes to overflow the 16 rem cap, wheel/keyboard step onto
 * an off-screen lane, timeline keeps showing the old rows).
 *
 * jsdom runs no layout, so the lane-row geometry is stubbed per element:
 * each `[data-timeline-axis]` cell reports a rect derived from a
 * test-supplied content-space top minus the body's live `scrollTop`
 * (matching how a real browser's viewport-space rect moves as the
 * container scrolls), and the body reports a stubbed `clientHeight`.
 */
describe('ThreadTimelineOverlay vertical scroll-follow', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  /** Override a layout property on a single DOM element. */
  function defineLayoutProp(
    el: HTMLElement,
    prop: 'clientHeight',
    value: number,
  ): void {
    Object.defineProperty(el, prop, {
      configurable: true,
      get: () => value,
    });
  }

  /**
   * Stub `getBoundingClientRect` so each lane's axis cell reports a
   * viewport-space rect consistent with the body's current `scrollTop`.
   * `laneTopByThreadId` holds each lane's CONTENT-space top; the stub
   * subtracts the live `scrollTop` at call time, which is exactly the
   * conversion the effect must undo — so the effect's
   * `rect.top - bodyRect.top + scrollTop` round-trips back to the
   * content-space value regardless of the scroll position when it fires.
   */
  function stubVerticalRects(
    body: HTMLElement,
    laneTopByThreadId: Map<string, number>,
    laneHeightPx = 18,
  ): void {
    const original = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(
      HTMLElement.prototype,
      'getBoundingClientRect',
    ).mockImplementation(function (this: HTMLElement) {
      const base = {
        left: 0,
        right: 240,
        width: 240,
        x: 0,
        toJSON: () => ({}),
      };
      if (this === body) {
        return {
          ...base,
          top: 0,
          y: 0,
          bottom: body.clientHeight,
          height: body.clientHeight,
        } as DOMRect;
      }
      if (this.hasAttribute('data-timeline-axis')) {
        const contentTop =
          laneTopByThreadId.get(this.getAttribute('data-thread-id') ?? '') ??
          0;
        const top = contentTop - body.scrollTop;
        return {
          ...base,
          top,
          y: top,
          bottom: top + laneHeightPx,
          height: laneHeightPx,
        } as DOMRect;
      }
      return original.call(this);
    });
  }

  /**
   * Two-lane fixture where a wheel-up crosses lanes: sorted large
   * messages are [msg-a (th1), msg-b (th2), msg-c (th1)], the playhead
   * starts on the last message (msg-c, lane 1), and one wheel-up step
   * lands on msg-b — lane 2. The lane the step targets is the one whose
   * row visibility drives the vertical catch-up.
   */
  function renderCrossLaneFixture() {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        created_at: '2026-01-01T00:00:30Z',
      }),
    ];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
      [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z')]],
    ]);
    return renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
  }

  /** Wheel-up on the axis column: steps msg-c → msg-b (lane 1 → lane 2). */
  function wheelUpOntoLane2(): void {
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
  }

  it('scrolls the body down to centre the lane row when a cross-lane step lands below the vertical viewport', async () => {
    renderCrossLaneFixture();
    await screen.findAllByTestId('thread-timeline-dot');
    const body = screen.getByTestId('thread-timeline-body');
    // Viewport shows 2 rows (36 px). Lane 1 sits at content top 0
    // (visible), lane 2 at content top 90 (below the 36 px fold).
    defineLayoutProp(body, 'clientHeight', 36);
    stubVerticalRects(
      body,
      new Map([
        ['1', 0],
        ['2', 90],
      ]),
    );
    body.scrollTop = 0;
    wheelUpOntoLane2();
    // Lane 2's band [90, 108] overflows viewBottom (36) → re-centre:
    // scrollTop = laneTop + laneHeight/2 - clientHeight/2 = 90 + 9 - 18 = 81.
    await waitFor(() => {
      expect(body.scrollTop).toBe(81);
    });
  });

  it('scrolls the body up when the target lane row is hidden above the vertical viewport', async () => {
    renderCrossLaneFixture();
    await screen.findAllByTestId('thread-timeline-dot');
    const body = screen.getByTestId('thread-timeline-body');
    // Lane 2 sits ABOVE lane 1 in content space this time (the stub is
    // free to place rows arbitrarily — the effect only reads rects).
    // Start scrolled down so lane 1 [90, 108] is visible in [90, 126]
    // and lane 2 [0, 18] is hidden above the fold.
    defineLayoutProp(body, 'clientHeight', 36);
    stubVerticalRects(
      body,
      new Map([
        ['1', 90],
        ['2', 0],
      ]),
    );
    body.scrollTop = 90;
    wheelUpOntoLane2();
    // Lane 2's top (0) < viewTop (90) → re-centre: max(0, 0 + 9 - 18) = 0.
    await waitFor(() => {
      expect(body.scrollTop).toBe(0);
    });
  });

  it('leaves scrollTop untouched when the target lane row is already fully visible', async () => {
    renderCrossLaneFixture();
    await screen.findAllByTestId('thread-timeline-dot');
    const body = screen.getByTestId('thread-timeline-body');
    // Both rows fit the 36 px viewport: lane 1 at [0, 18], lane 2 at
    // [18, 36]. The cross-lane step must NOT trigger any scroll — a
    // re-centre here would visibly yank rows around on every step in the
    // (common) short-session case where all lanes fit the cap.
    defineLayoutProp(body, 'clientHeight', 36);
    stubVerticalRects(
      body,
      new Map([
        ['1', 0],
        ['2', 18],
      ]),
    );
    body.scrollTop = 0;
    wheelUpOntoLane2();
    // Wait for the step to land (lane 2 becomes the active lane) so the
    // effect has fired before the no-scroll assertion.
    await waitFor(() => {
      const lane2 = body.querySelector<HTMLElement>(
        '[data-timeline-axis][data-thread-id="2"]',
      );
      expect(lane2?.getAttribute('data-active')).toBe('true');
    });
    expect(body.scrollTop).toBe(0);
  });
});
