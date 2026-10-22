/**
 * Initial playhead placement: showing the timeline scrolls the axis
 * instantly so the auto-anchored playhead (the latest message, at the far
 * right of a long axis) lands inside the visible axis band, without any
 * navigation, and only once per showing.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useNavStore } from '../../store/navStore';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
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

let resizeObservers: ControllableResizeObservers;

beforeEach(() => {
  resetGlobals();
  resizeObservers = installControllableResizeObserver();
});

afterEach(() => {
  resizeObservers.restore();
});

/** Width of the sticky label column the tests stub (px). */
const LABEL_WIDTH = 60;
/** msg-c, the auto-anchored latest message, sits at x = 240 on the axis. */
const LATEST_PLAYHEAD_IN_CONTENT = LABEL_WIDTH + 240 + LANE_LEFT_PAD_PX; // = 316

function messagesFixture() {
  return new Map([
    [
      1,
      [
        makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
        makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
        makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
      ],
    ],
  ]);
}

function renderTimeline(
  options: {
    messagesByThread?: ReturnType<typeof messagesFixture>;
    beforeFetch?: () => Promise<void>;
  } = {},
) {
  stubAxisRect({ left: 0, width: 240 });
  return renderOverlay({
    threads: [makeThread(1)],
    messagesByThread: options.messagesByThread ?? messagesFixture(),
    activeThreadId: 1,
    conversationArticles: [
      { uuid: 'msg-a' },
      { uuid: 'msg-b' },
      { uuid: 'msg-c' },
    ],
    beforeFetch: options.beforeFetch,
  });
}

/**
 * Give the freshly mounted axis-scroll container its label offset and spy on
 * its `scrollTo`, then deliver its first width measurement.
 */
function measureAxisColumn(width: number) {
  const wrapper = screen.getByTestId('thread-timeline-axis-column');
  const axisEl = wrapper.querySelector<HTMLElement>('[data-timeline-axis]')!;
  defineLayoutProp(axisEl, 'offsetLeft', LABEL_WIDTH);
  const scrollTo = vi.spyOn(wrapper, 'scrollTo');
  resizeObservers.resize(wrapper, width);
  return { wrapper, scrollTo };
}

/** The scrollLeft that centres the playhead in the visible axis band. */
function bandCentredScrollLeft(viewportWidth: number): number {
  return (
    LATEST_PLAYHEAD_IN_CONTENT -
    (LABEL_WIDTH + (viewportWidth - LABEL_WIDTH) / 2)
  );
}

function expectInsideBand(wrapper: HTMLElement, viewportWidth: number) {
  const viewportX = LATEST_PLAYHEAD_IN_CONTENT - wrapper.scrollLeft;
  expect(viewportX).toBeGreaterThan(LABEL_WIDTH);
  expect(viewportX).toBeLessThan(viewportWidth);
}

describe('ThreadTimelineOverlay initial playhead placement', () => {
  it('scrolls the axis instantly so the playhead lands in the visible axis band when the timeline is expanded, and again after a collapse and re-expand', async () => {
    renderTimeline();
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

    // A 200 px viewport: the playhead at content x = 316 lies past the
    // right edge while the axis sits at scrollLeft 0.
    const first = measureAxisColumn(200);
    await waitFor(() => {
      expect(first.scrollTo).toHaveBeenCalledWith({
        left: bandCentredScrollLeft(200),
        behavior: 'auto',
      });
    });
    expect(first.scrollTo).toHaveBeenCalledTimes(1);
    expectInsideBand(first.wrapper, 200);

    // Collapse and re-expand: the new scroll container starts at 0 again
    // and gets the same placement.
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(screen.queryByTestId('thread-timeline-axis-column')).toBeNull();
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const second = measureAxisColumn(200);
    expect(second.wrapper).not.toBe(first.wrapper);
    await waitFor(() => {
      expect(second.scrollTo).toHaveBeenCalledWith({
        left: bandCentredScrollLeft(200),
        behavior: 'auto',
      });
    });
    expectInsideBand(second.wrapper, 200);
  });

  it('places the playhead once the lane messages arrive after the first width measurement', async () => {
    let releaseFetch!: () => void;
    const fetchGate = new Promise<void>((resolve) => {
      releaseFetch = resolve;
    });
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    renderTimeline({ beforeFetch: () => fetchGate });

    // The axis-scroll container is mounted (one lane per thread) but its
    // messages are still in flight when the width is first measured.
    const { wrapper, scrollTo } = measureAxisColumn(200);
    expect(screen.queryAllByTestId('thread-timeline-dot')).toHaveLength(0);
    expect(scrollTo).not.toHaveBeenCalled();

    await act(async () => {
      releaseFetch();
    });
    await screen.findAllByTestId('thread-timeline-dot');
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalledWith({
        left: bandCentredScrollLeft(200),
        behavior: 'auto',
      });
    });
    expect(scrollTo).toHaveBeenCalledTimes(1);
    expectInsideBand(wrapper, 200);
  });

  it('does not scroll on showing when the playhead already lies inside the visible axis band', async () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    renderTimeline();
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

    // A 600 px viewport: the band with its margin is [180, 480] and the
    // playhead at content x = 316 already sits inside it.
    const { wrapper, scrollTo } = measureAxisColumn(600);
    // Let any pending effect flush before asserting the negative.
    await act(async () => {});
    expect(scrollTo).not.toHaveBeenCalled();
    expect(wrapper.scrollLeft).toBe(0);
  });

  it('does not scroll the axis again when a background refetch changes the message list after the initial placement', async () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    const messagesByThread = messagesFixture();
    const { queryClient } = renderTimeline({ messagesByThread });
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const { wrapper, scrollTo } = measureAxisColumn(200);
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalledTimes(1);
    });

    // The user drags the scrollbar back to the start of the axis.
    wrapper.scrollLeft = 0;

    // A background refetch appends a message: the playhead follows it to
    // the new latest mark and the x map is rebuilt, but no navigation or
    // resize happened, so the scroll position the user chose stays.
    messagesByThread.set(1, [
      ...messagesByThread.get(1)!,
      makeUserText(1, 3, 'msg-d', '2026-01-01T00:03:00Z'),
    ]);
    await act(async () => {
      await queryClient.refetchQueries();
    });
    await waitFor(() => {
      expect(screen.getAllByTestId('thread-timeline-dot')).toHaveLength(4);
    });
    await act(async () => {});
    expect(scrollTo).toHaveBeenCalledTimes(1);
    expect(wrapper.scrollLeft).toBe(0);
  });

  it('places the playhead again when the focused session changes under a timeline that stays expanded', async () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    window.localStorage.setItem(timelineExpandedKey('session-2'), 'true');
    renderTimeline();
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const { wrapper, scrollTo } = measureAxisColumn(200);
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalledTimes(1);
    });
    wrapper.scrollLeft = 0;

    // Both sessions keep the timeline expanded, so the scroll container
    // stays mounted across the switch and its width never returns to 0.
    act(() => {
      useNavStore.setState({ focusedSessionId: 'session-2' });
    });
    expect(screen.getByTestId('thread-timeline-axis-column')).toBe(wrapper);
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalledTimes(2);
    });
    expect(scrollTo).toHaveBeenLastCalledWith({
      left: bandCentredScrollLeft(200),
      behavior: 'auto',
    });
    expectInsideBand(wrapper, 200);
  });
});
