/**
 * Collapse toggle, jump-to-edge buttons, collapsed query gating,
 * and lane labels.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import {
  fireEvent,
  screen,
  waitFor,
} from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { useNavStore } from '../../store/navStore';
import { LANE_LEFT_PAD_PX } from './ThreadTimelineOverlay';
import {
  makeThread,
  makeUserText,
  playheadLeftPx,
  renderOverlay,
  resetGlobals,
  TEST_SESSION_ID,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay collapse toggle', () => {
  beforeEach(() => {
    resetGlobals();
  });

  it('starts collapsed when no preference has been saved', () => {
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    const toggle = screen.getByTestId('thread-timeline-toggle');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('thread-timeline-body')).toBeNull();
  });

  it('labels the collapsed toggle "Timeline" with a leading icon, matching the Terminal button shape', () => {
    // The collapsed toggle reads "Timeline" (short, paired with an icon)
    // rather than "Thread timeline" so it sits visually balanced beside
    // the Terminal toggle in the transcript pane's top region. The icon
    // is an inline SVG (no icon library is used in this codebase).
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    const toggle = screen.getByTestId('thread-timeline-toggle');
    expect(toggle).toHaveAttribute('aria-label', 'Timeline');
    expect(toggle).toHaveTextContent('Timeline');
    expect(toggle).not.toHaveTextContent('Thread timeline');
    // The leading glyph is an inline SVG. Querying by selector is the
    // cleanest way (no semantic role for decorative icons).
    expect(toggle.querySelector('svg')).not.toBeNull();
  });

  it('toggles open on click and persists the preference', () => {
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('thread-timeline-body')).toBeInTheDocument();
    expect(window.localStorage.getItem(timelineExpandedKey())).toBe(
      'true',
    );
  });

  it('restores the persisted expanded preference on mount', () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
  });

  it('toggles closed again and persists the change', () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');
    expect(window.localStorage.getItem(timelineExpandedKey())).toBe(
      'false',
    );
  });

  it('keeps the expand preference independent across sessions (no cross-talk)', () => {
    // The preference is per session, not device-global: one session can be
    // expanded while another stays collapsed. A regression that reverts to
    // a single device-wide key would break this case — toggling under
    // session A would suddenly affect session B's restored state.
    const OTHER_SESSION = 'session-other';

    // Seed session A's preference to expanded. Session B has no preference,
    // so its restored state must be the default (collapsed).
    window.localStorage.setItem(timelineExpandedKey(), 'true');

    // Render once with session A's id focused: expanded.
    const { unmount } = renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: new Map(),
    });
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
    unmount();

    // Switch focus to a different session id, with no preference written
    // for it. The overlay must mount collapsed — session B does not inherit
    // session A's expand state.
    useNavStore.setState({ focusedSessionId: OTHER_SESSION });
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');

    // And session A's localStorage entry is still intact — switching
    // session does not clobber the other's preference.
    expect(window.localStorage.getItem(timelineExpandedKey(TEST_SESSION_ID))).toBe(
      'true',
    );
    expect(
      window.localStorage.getItem(timelineExpandedKey(OTHER_SESSION)),
    ).toBeNull();
  });
});

describe('ThreadTimelineOverlay jump-to-edge buttons', () => {
  beforeEach(() => {
    resetGlobals();
  });

  it('renders both jump buttons in the expanded header', () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    const start = screen.getByTestId('thread-timeline-jump-start');
    const end = screen.getByTestId('thread-timeline-jump-end');
    expect(start).toHaveAttribute('aria-label', 'Jump to timeline start');
    expect(end).toHaveAttribute('aria-label', 'Jump to timeline end');
    // Both buttons are real <button>s, not nested inside the toggle — so
    // clicking either one does not flip aria-expanded (see the dedicated
    // case below). Each renders its own decorative SVG glyph.
    expect(start.tagName).toBe('BUTTON');
    expect(end.tagName).toBe('BUTTON');
    expect(start.querySelector('svg')).not.toBeNull();
    expect(end.querySelector('svg')).not.toBeNull();
  });

  it('omits both jump buttons in the collapsed state', () => {
    // Collapsed default: the floating pill is the only control, no jump
    // buttons. The jump buttons live inside the expanded header card.
    renderOverlay({ threads: [makeThread(1)], messagesByThread: new Map() });
    expect(screen.queryByTestId('thread-timeline-jump-start')).toBeNull();
    expect(screen.queryByTestId('thread-timeline-jump-end')).toBeNull();
  });

  it('jumps the playhead to the first message on jump-start click', async () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
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
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }, { uuid: 'msg-c' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead anchors to the active lane’s latest large turn
    // (msg-c, x=1 → 240px).
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);

    // Click jump-start: the playhead snaps to msg-a (x=0).
    fireEvent.click(screen.getByTestId('thread-timeline-jump-start'));
    await waitFor(() =>
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${0 + LANE_LEFT_PAD_PX}px`),
    );
  });

  it('jumps the playhead to the last message on jump-end click', async () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
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
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }, { uuid: 'msg-c' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Move off the latest first by clicking jump-start, so jump-end's effect
    // is observable (the initial settle is already at the last message).
    fireEvent.click(screen.getByTestId('thread-timeline-jump-start'));
    await waitFor(() =>
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${0 + LANE_LEFT_PAD_PX}px`),
    );
    fireEvent.click(screen.getByTestId('thread-timeline-jump-end'));
    await waitFor(() =>
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`),
    );
  });

  it('keeps the timeline expanded when either jump button is clicked', async () => {
    // The jump buttons live OUTSIDE the toggle button, so a click on them
    // must not bubble into a collapse. Both the aria state and the body
    // testid have to stay put across consecutive clicks.
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(screen.getByTestId('thread-timeline-jump-start'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('thread-timeline-body')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('thread-timeline-jump-end'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('thread-timeline-body')).toBeInTheDocument();
  });

  it('disables both jump buttons when there are no messages', () => {
    // No threads => `sortedMessages` is empty, so there is nowhere to jump.
    // The buttons render dimmed and refuse clicks (via the `disabled`
    // attribute) rather than silently no-op'ing — clearer affordance.
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    renderOverlay({ threads: [], messagesByThread: new Map() });
    const start = screen.getByTestId('thread-timeline-jump-start');
    const end = screen.getByTestId('thread-timeline-jump-end');
    expect(start).toBeDisabled();
    expect(end).toBeDisabled();
  });
});

/**
 * The collapsed overlay still mounts `useThreadsMessagesQueries` (so the
 * `expanded` -> enabled transition lights it up without remount churn), but
 * the per-thread fan-out must stay quiet until the user actually expands.
 *
 * Cold-load motivation: the browser caps at six HTTP/1.1 connections per host;
 * an unconditional fan-out across many threads saturates the pool and stretches
 * the focused-thread load that sits behind it. The fetched-per-thread state
 * here is asserted on the mock `getThreadMessages`, not on a fetched-array
 * reference, so the test stays insensitive to TanStack Query's internal
 * `fetchStatus` plumbing.
 */
describe('ThreadTimelineOverlay collapsed query gating', () => {
  beforeEach(() => {
    resetGlobals();
  });

  it('does not fetch per-thread messages while collapsed', async () => {
    const threads = [makeThread(1), makeThread(2), makeThread(3)];
    const { apiClient } = renderOverlay({
      threads,
      messagesByThread: new Map(),
    });
    // The hook is mounted, but enabled=false: no thread-messages request fires.
    // Flush microtasks so any (incorrect) auto-fetch would have shown up.
    await Promise.resolve();
    await Promise.resolve();
    expect(apiClient.getThreadMessages).not.toHaveBeenCalled();
  });

  it('fetches per-thread messages once expanded by the user', async () => {
    const threads = [makeThread(1), makeThread(2), makeThread(3)];
    const { apiClient } = renderOverlay({
      threads,
      messagesByThread: new Map(),
    });
    expect(apiClient.getThreadMessages).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    await waitFor(() => {
      expect(apiClient.getThreadMessages).toHaveBeenCalledTimes(threads.length);
    });
    const calledIds = vi
      .mocked(apiClient.getThreadMessages)
      .mock.calls.map((call) => call[0]);
    expect(new Set(calledIds)).toEqual(new Set([1, 2, 3]));
  });

  it('fetches all threads on mount when the expanded preference is restored', async () => {
    window.localStorage.setItem(timelineExpandedKey(), 'true');
    const threads = [makeThread(1), makeThread(2)];
    const { apiClient } = renderOverlay({
      threads,
      messagesByThread: new Map(),
    });
    await waitFor(() => {
      expect(apiClient.getThreadMessages).toHaveBeenCalledTimes(threads.length);
    });
  });
});

describe('ThreadTimelineOverlay lane labels', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it("uses the wire thread.title as the lane label, matching Navigator", async () => {
    const subTitle = 'Investigate the staging migration failure end to end';
    const threads = [
      makeThread(1, {
        // The main thread's wire title is typically the session prompt; the
        // lane labels it `"main"` regardless, mirroring Navigator's omission
        // of the main row and the breadcrumb's left-most "main" crumb.
        title: 'a long session prompt the server stored here',
        created_at: '2026-01-01T00:00:00Z',
      }),
      makeThread(2, {
        title: subTitle,
        parent_thread_id: 1,
        root_message_uuid: 'root-of-sub',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const labels = await screen.findAllByTestId('thread-timeline-lane-label');
    expect(labels[0]).toHaveTextContent('main');
    expect(labels[0]).toHaveAttribute('title', 'main');
    // Sub-thread label is the wire title verbatim; CSS `truncate` shortens
    // visually but the full title remains in the DOM and in the tooltip.
    expect(labels[1]).toHaveTextContent(subTitle);
    expect(labels[1]).toHaveAttribute('title', subTitle);
  });

  it('falls back to `thread <id>` when the wire title is empty', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        title: '',
        parent_thread_id: 1,
        root_message_uuid: 'root-uuid-only',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const labels = await screen.findAllByTestId('thread-timeline-lane-label');
    expect(labels[1]).toHaveTextContent('thread 2');
    expect(labels[1]).toHaveAttribute('title', 'thread 2');
  });
});
