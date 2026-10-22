/**
 * Long lane labels on a narrow timeline: the label column is capped at a
 * fraction of the visible width, the horizontal catch-up lands the playhead
 * right of the sticky label column, and a resize brings a hidden playhead
 * back into view.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import { act, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
import { LANE_LABEL_MAX_WIDTH_FRACTION } from './ThreadTimelineOverlay';
import {
  defineLayoutProp,
  installControllableResizeObserver,
  makeThread,
  makeUserText,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
  type ControllableResizeObservers,
} from './ThreadTimelineOverlay.testkit';

const LONG_TITLE =
  'a subthread whose name is far longer than the visible width of the timeline card on a narrow window';

/**
 * jsdom runs no layout, so the axis-scroll container's width and the axis
 * cell's offset are stubbed per element, and the global inert
 * `ResizeObserver` is replaced by one whose callbacks the test fires by hand.
 */
let resizeObservers: ControllableResizeObservers;

beforeEach(() => {
  resetGlobals();
  window.localStorage.setItem(timelineExpandedKey(), 'true');
  resizeObservers = installControllableResizeObserver();
});

afterEach(() => {
  resizeObservers.restore();
});

/** Three large messages on one long-named subthread under `main`. */
function renderLongLabelTimeline() {
  stubAxisRect({ left: 0, width: 240 });
  const threads = [
    makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
    makeThread(2, {
      title: LONG_TITLE,
      parent_thread_id: 1,
      created_at: '2026-01-01T00:00:30Z',
    }),
  ];
  const messages = new Map([
    [
      2,
      [
        makeUserText(2, 0, 'msg-a', '2026-01-01T00:00:00Z'),
        makeUserText(2, 1, 'msg-b', '2026-01-01T00:01:00Z'),
        makeUserText(2, 2, 'msg-c', '2026-01-01T00:02:00Z'),
      ],
    ],
  ]);
  renderOverlay({
    threads,
    messagesByThread: messages,
    activeThreadId: 2,
    conversationArticles: [
      { uuid: 'msg-a' },
      { uuid: 'msg-b' },
      { uuid: 'msg-c' },
    ],
  });
}

describe('ThreadTimelineOverlay long lane labels', () => {
  it('caps every lane label at the configured fraction of the axis-scroll width and truncates the text with an ellipsis', async () => {
    renderLongLabelTimeline();
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    resizeObservers.resize(wrapper, 300);

    const expectedCapPx = Math.floor(300 * LANE_LABEL_MAX_WIDTH_FRACTION);
    const labels = screen.getAllByTestId('thread-timeline-lane-label');
    expect(labels).toHaveLength(2);
    // Every label shares the same cap, so the label column — and every
    // lane's playhead segment — stays aligned across rows.
    for (const label of labels) {
      expect(label.style.maxWidth).toBe(`${expectedCapPx}px`);
      expect(label.className).toContain('overflow-hidden');
    }
    const longLabel = labels.find((l) => l.dataset.threadId === '2')!;
    // The label cell is a flex container, so the ellipsis has to come
    // from an inner text element that truncates.
    const text = longLabel.querySelector('span');
    expect(text).not.toBeNull();
    expect(text!.className).toContain('truncate');
    expect(text!.textContent).toBe(LONG_TITLE);
    // The full name stays reachable through the tooltip.
    expect(longLabel.getAttribute('title')).toContain(LONG_TITLE);

    // The cap follows the card width.
    resizeObservers.resize(wrapper, 500);
    for (const label of screen.getAllByTestId('thread-timeline-lane-label')) {
      expect(label.style.maxWidth).toBe(
        `${Math.floor(500 * LANE_LABEL_MAX_WIDTH_FRACTION)}px`,
      );
    }
  });

  it('lands the playhead in the visible axis band, not under the label, when the label column is wider than half the viewport', async () => {
    renderLongLabelTimeline();
    await screen.findAllByTestId('thread-timeline-dot');
    // The mount anchor puts the playhead on msg-c (x = 240 on the axis).
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]')!;
    // Label column (160 px) wider than half the 300 px viewport.
    const viewportWidth = 300;
    const labelWidth = 160;
    defineLayoutProp(wrapper, 'clientWidth', viewportWidth);
    defineLayoutProp(axisEl, 'offsetLeft', labelWidth);
    // msg-b paints at content x = 160 + 120 + 16 = 296. With scrollLeft
    // 200 its viewport-x is 96, under the sticky label, so the catch-up
    // fires after the wheel step.
    wrapper.scrollLeft = 200;
    act(() => {
      wrapper.dispatchEvent(
        new WheelEvent('wheel', { deltaY: -100, bubbles: true, cancelable: true }),
      );
    });
    const playheadInContent = labelWidth + 120 + LANE_LEFT_PAD_PX; // = 296
    await waitFor(() => {
      // Centring on the whole viewport would give 296 - 150 = 146, leaving
      // the playhead at viewport-x 150, under the 160 px label.
      expect(wrapper.scrollLeft).toBe(
        playheadInContent - (labelWidth + (viewportWidth - labelWidth) / 2),
      ); // = 66
    });
    const playheadViewportX = playheadInContent - wrapper.scrollLeft;
    expect(playheadViewportX).toBeGreaterThan(labelWidth);
    expect(playheadViewportX).toBeLessThan(viewportWidth);
  });

  it('scrolls the playhead back into the visible axis band when the axis-scroll container shrinks, without a new navigation', async () => {
    renderLongLabelTimeline();
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]')!;
    wrapper.scrollLeft = 0;

    // Wide card: the playhead (content x = 240 + 240 + 16 = 496) already
    // sits inside the visible axis band, so the initial placement on the
    // first measurement leaves the scroll position alone.
    defineLayoutProp(axisEl, 'offsetLeft', 240);
    resizeObservers.resize(wrapper, 800);
    expect(wrapper.scrollLeft).toBe(0);

    // Narrow the card: the capped label column shrinks to 120 px and msg-c
    // (content x = 120 + 240 + 16 = 376) now sits past the right edge of the
    // 300 px viewport.
    defineLayoutProp(axisEl, 'offsetLeft', 120);
    resizeObservers.resize(wrapper, 300);
    const playheadInContent = 120 + 240 + LANE_LEFT_PAD_PX; // = 376
    await waitFor(() => {
      expect(wrapper.scrollLeft).toBe(
        playheadInContent - (120 + (300 - 120) / 2),
      ); // = 166
    });
    const playheadViewportX = playheadInContent - wrapper.scrollLeft;
    expect(playheadViewportX).toBeGreaterThan(120);
    expect(playheadViewportX).toBeLessThan(300);
  });
});
