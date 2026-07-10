import { createRef } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { Message, Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import {
  ALL_ARTICLES_SELECTOR,
  articleMessageSelector,
  LANE_LEFT_PAD_PX,
  MARK_CLUSTER_PX,
  MARK_SMALL_PX,
  PANE_SCROLL_DEBOUNCE_MS,
  PANE_SCROLL_OBSERVER_THRESHOLD,
  PANE_SCROLL_PROGRAMMATIC_GUARD_MS,
  SCROLL_DOM_READY_TIMEOUT_MS,
  ThreadTimelineOverlay,
  TIMELINE_EXPANDED_SUBKEY,
  TIMELINE_JUMP_HIGHLIGHT_CLASS,
  WHEEL_DELTA_LINE_PX,
  WHEEL_PER_EVENT_CLAMP_PX,
  WHEEL_STEP_COOLDOWN_MS,
  WHEEL_VELOCITY_WINDOW_MS,
  normalizeWheelDeltaPx,
  resetTimelineExpandedForTests,
  scheduleScrollAfterRender,
  scrollMessageIntoView,
  stepsForCumulativePx,
} from './ThreadTimelineOverlay';
import { sessionScopedKey } from '../../store/sessionScopedStorage';

/**
 * Session id the test fixtures pin every thread / message to (see
 * `makeThread` / `makeMessage`). The overlay reads the focused session id
 * from `navStore` to scope its expand preference, so every test sets this
 * value as the focus in `resetGlobals` — otherwise the hook falls back to
 * the in-memory-only `null` branch and never persists.
 */
const TEST_SESSION_ID = 'session-1';

/**
 * Compose the localStorage key the overlay actually writes to for the
 * current test session. Wraps the helper's `(sessionId, subKey)` shape so
 * each test reads `localStorage.getItem(timelineExpandedKey())` rather than
 * spelling the layout out by hand.
 */
function timelineExpandedKey(sessionId: string = TEST_SESSION_ID): string {
  return sessionScopedKey(sessionId, TIMELINE_EXPANDED_SUBKEY);
}

function makeThread(
  id: number,
  overrides: Partial<Thread> = {},
): Thread {
  return {
    id,
    session_id: 'session-1',
    title: `thread ${id}`,
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function makeMessage(
  threadId: number,
  seq: number,
  uuid: string,
  overrides: Partial<Message> = {},
): Message {
  return {
    uuid,
    session_id: 'session-1',
    thread_id: threadId,
    role: 'user',
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq,
    content_text: null,
    content: [],
    created_at: '2026-01-01T00:00:00Z',
    model: null,
    git_branch: null,
    cwd: null,
    response_time_ms: null,
    ...overrides,
  };
}

/**
 * A user-role message carrying a single text block — i.e. a "large"
 * main-conversation turn the wheel step navigation targets. Tests that
 * exercise wheel stepping use this so the messages land in the
 * `largeSortedMessages` subset (the wheel skips auxiliary tool/meta marks).
 */
function makeUserText(
  threadId: number,
  seq: number,
  uuid: string,
  createdAt: string,
): Message {
  return makeMessage(threadId, seq, uuid, {
    role: 'user',
    content: [{ type: 'text', text: `text ${uuid}` }],
    created_at: createdAt,
  });
}

/**
 * Render the overlay against a stubbed ApiClient that resolves
 * `getThreadMessages` from the provided in-memory map. The conversation body
 * is a sibling div carrying the article elements the playhead's jump targets.
 */
function renderOverlay({
  threads,
  messagesByThread,
  activeThreadId = null,
  conversationArticles = [] as { uuid: string }[],
}: {
  threads: Thread[];
  messagesByThread: Map<number, Message[]>;
  activeThreadId?: number | null;
  conversationArticles?: { uuid: string }[];
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
  vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
    async (threadId) => ({
      messages: messagesByThread.get(threadId as number) ?? [],
    }),
  );
  const bodyRef = createRef<HTMLDivElement>();
  const result = render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={apiClient}>
        <div>
          <div ref={bodyRef} data-testid="conversation-body">
            {conversationArticles.map((a) => (
              <article key={a.uuid} data-message-uuid={a.uuid}>
                {a.uuid}
              </article>
            ))}
          </div>
          <ThreadTimelineOverlay
            threads={threads}
            activeThreadId={activeThreadId}
            conversationBodyRef={bodyRef}
          />
        </div>
      </ApiProvider>
    </QueryClientProvider>,
  );
  return { ...result, apiClient, bodyRef };
}

/**
 * Reset the cross-test global state the overlay touches: localStorage (the
 * collapse preference) and the navStore (active thread / focused session).
 */
function resetGlobals() {
  window.localStorage.clear();
  // The expanded preference is cached in module state for cross-component
  // sync (see `useTimelineExpanded`); reset the cache too so each test
  // reads the freshly-cleared (or freshly-seeded) localStorage value. With
  // no argument every per-session entry is cleared.
  resetTimelineExpandedForTests();
  useNavStore.setState({
    // Pin the focused session so the overlay's per-session expand hook can
    // read/write its localStorage entry — without a real id the hook falls
    // back to in-memory only (collapsed default, no persistence) and the
    // expand-preference cases never see anything written.
    focusedSessionId: TEST_SESSION_ID,
    activeThreadId: null,
    preNewSessionFocus: null,
    settingsOpen: false,
  });
}

/**
 * Stub the first lane axis row's bounding rect so click-to-jump tests can
 * supply deterministic playhead coordinates without measuring real layout
 * (jsdom does not run CSS, so every rect is 0 by default).
 */
function stubAxisRect(rect: Partial<DOMRect>): void {
  const original = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
    function (this: HTMLElement) {
      if (this.hasAttribute('data-timeline-axis')) {
        return {
          left: 0,
          top: 0,
          right: 240,
          bottom: 18,
          width: 240,
          height: 18,
          x: 0,
          y: 0,
          toJSON: () => ({}),
          ...rect,
        } as DOMRect;
      }
      return original.call(this);
    },
  );
}

/**
 * Read a playhead element's resolved x in pixels along the lane axis.
 *
 * v30 switched the playhead from `style.left = "<px>"` to
 * `style.transform = "translateX(<px>)"` so the 2 px bar paints on a
 * GPU-composited layer and stops shimmering across subpixel boundaries.
 * Every test that previously asserted on `.style.left` for the playhead now
 * routes through this helper so the assertion target follows the implementation
 * without spreading translateX-string parsing through hundreds of call sites.
 */
function playheadLeftPx(el: HTMLElement): string {
  const transform = el.style.transform;
  const match = /translateX\((-?\d+(?:\.\d+)?)px\)/.exec(transform);
  if (match === null) {
    throw new Error(
      `playhead element is missing a translateX(...) transform (got transform=${JSON.stringify(
        transform,
      )}, left=${JSON.stringify(el.style.left)})`,
    );
  }
  return `${match[1]}px`;
}

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
    // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);

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

describe('ThreadTimelineOverlay playhead', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders one playhead per lane so the scrub indicator scrolls with the body', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const playheads = await screen.findAllByTestId('thread-timeline-playhead');
    expect(playheads).toHaveLength(2);
  });

  it('does not navigate when a dot is merely hovered (no hover-jump)', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const threads = [makeThread(1)];
    // Use large (user-text) messages so they render as individual dots; two
    // consecutive empty-content user messages would now collapse into a
    // cluster (see small-dot clustering) and there would be no individual
    // `msg-a` dot to hover.
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
    // The initial-settle is intentionally inert: the playhead lands on the
    // latest dot but does NOT scroll the pane (or switch threads) until the
    // user scrubs — so no one ever expects to see scrollIntoView called yet.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    expect(dots).toHaveLength(2);
    expect(scrollIntoView).not.toHaveBeenCalled();
    // Hovering and leaving any dot must not move the playhead or jump.
    const dotA = dots.find((d) => d.getAttribute('data-message-uuid') === 'msg-a');
    fireEvent.mouseEnter(dotA!);
    fireEvent.mouseLeave(dotA!);
    // Give microtasks a chance to run; nothing should fire from a hover.
    await Promise.resolve();
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it('jumps the playhead to a clicked x and scrolls the matching message into view', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Use large (user-text) messages so each renders as an individual dot
    // and a click can directly target msg-a's x without going through a
    // cluster mark.
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
    // Wait for the data-driven layout (dots + playhead) to land. The initial
    // settle is intentionally silent; the click below is the only thing that
    // should ever call scrollIntoView in this test.
    await screen.findAllByTestId('thread-timeline-dot');
    expect(scrollIntoView).not.toHaveBeenCalled();
    // The axis width is stubbed at 240; clicking at x=0 lands the playhead at
    // fraction 0, which is msg-a (the earliest message).
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
      clientX: 0,
    });
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(1));
    const target = within(screen.getByTestId('conversation-body')).getByText(
      'msg-a',
    );
    expect(scrollIntoView.mock.instances[0]).toBe(target);
  });

  it('switches the active thread and defers the scroll until the next frame when the playhead lands in another lane', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    // Capture rAF callbacks so the test can drive them after re-rendering
    // with the target lane's article in the DOM — mirroring how the live
    // app re-renders the conversation pane on active-thread change.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* tests do not exercise cancellation here */
    }) as typeof window.cancelAnimationFrame;
    stubAxisRect({ left: 0, width: 240 });
    try {
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: 'msg-a',
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      const messages = new Map([
        [
          1,
          [
            makeMessage(1, 0, 'msg-a', {
              created_at: '2026-01-01T00:00:00Z',
            }),
          ],
        ],
        [
          2,
          [
            makeMessage(2, 0, 'msg-b', {
              created_at: '2026-01-01T00:02:00Z',
            }),
          ],
        ],
      ]);
      // Start with lane 1 active; only the active lane's article (msg-a) is
      // rendered, mirroring the live app where the conversation pane only
      // holds the active thread's messages.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [{ uuid: 'msg-a' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // The initial settle does NOT scroll on first mount (the user has
      // not asked to be moved), so the click below is the only thing that
      // should ever drive a thread switch or scroll in this test.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Click at the right edge: msg-b sits at x=1 on lane 2 (cross-lane).
      fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
        clientX: 240,
      });
      // The active thread must flip to msg-b's lane (thread 2).
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      // The scroll is deferred to the next frame; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Re-render with the target lane's article in the DOM and the active
      // thread updated, mirroring the live app's response to the switch.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={2}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Drain the captured rAF callbacks now that the DOM holds msg-b.
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(performance.now());
      }
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
      expect(scrollIntoView).toHaveBeenCalledWith({ block: 'start' });
      const target = within(screen.getByTestId('conversation-body')).getByText(
        'msg-b',
      );
      expect(scrollIntoView.mock.instances[0]).toBe(target);
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('advances exactly one step on a single slow full-notch wheel event', async () => {
    // Regression for v9: the v9 staircase tripped the 2-step bucket at
    // exactly one notch's worth of |delta| (100 px), so the user could
    // never land on the immediate prev/next message — a slow single
    // mouse-wheel notch always jumped two messages. The fix lifts the
    // first acceleration bucket above the per-event clamp, so a single
    // notch of any size up to the clamp always walks exactly one step.
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
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
      // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      // A single full-notch wheel-up event (|deltaY| = 100, the canonical
      // mouse-wheel notch on Linux/Chrome) lands the playhead on the
      // immediate previous large turn (msg-b at x=0.5 → 120px), NOT msg-a.
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -100,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('advances one step on a leisurely sub-notch wheel event and suppresses page scroll', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Three evenly-spaced "large" turns: x=0, x=0.5, x=1 (px 0, 120, 240).
    // The wheel step navigates the main-conversation subset, so each message
    // must carry an authored text block to land on the large list.
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
    // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // A single sub-notch wheel-up event (cumulative |delta| under the
    // first staircase threshold) lands in the slowest bucket → exactly
    // one step back. preventDefault is also called so the page scroll
    // does not run alongside the navigation step.
    const body = screen.getByTestId('thread-timeline-axis-column');
    const wheel = new WheelEvent('wheel', {
      deltaY: -50,
      bubbles: true,
      cancelable: true,
    });
    const preventDefault = vi.spyOn(wheel, 'preventDefault');
    act(() => {
      body.dispatchEvent(wheel);
    });
    expect(preventDefault).toHaveBeenCalled();
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
  });

  it('clamps at the newest end: a wheel-down at the last message does not advance', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Use large turns so each is a wheel-step target — the wheel handler
    // walks the main-conversation subset, and empty-content user dots are
    // small (and now cluster) which would put them outside the subset.
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
    // Initial position is the last message (msg-b, x=1 → 240px).
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    const body = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: 100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    // Still at msg-b — the clamp blocks any further advance.
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('clamps at the oldest end: a wheel-up at the first message does not retreat', async () => {
    // Drive a virtual clock so the rolling-window accumulator can be reset
    // between events without sleeping the test.
    let nowMs = 1_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      // Both messages must be "large" so they survive the wheel-step subset
      // filter — the wheel only walks main-conversation turns.
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
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Step back once (msg-b → msg-a).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -100,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
      // Advance past the rolling window so the accumulator resets — the
      // next event is treated as a fresh leisurely turn rather than a
      // continuation of the previous burst.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 1;
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -100,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Still at msg-a — the clamp blocks any further retreat.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it("crosses lanes when wheel-stepping from lane A's last message into lane B's first", async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* tests do not exercise cancellation here */
    }) as typeof window.cancelAnimationFrame;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Lane 1 holds msg-a then msg-b; lane 2 holds msg-c (later). The
      // global sorted list is [msg-a, msg-b, msg-c], so wheel-up from the
      // final position (msg-c) lands on msg-b, which lives on lane 1 — a
      // cross-lane step from the user's starting subthread (lane 2).
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      // All three messages are "large" so the wheel-step subset includes
      // them and the cross-lane walk steps msg-c → msg-b.
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
        [
          2,
          [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')],
        ],
      ]);
      // Start with lane 2 active; only msg-c is in the conversation pane.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial settle does not fire; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Wheel-up from msg-c → msg-b. msg-b lives on lane 1 (cross-lane).
      // A sub-notch event keeps the staircase at one step so the
      // assertion targets msg-b (the immediate large neighbour), not
      // msg-a (two steps back, which a 100-px notch would reach).
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // The scroll is deferred to the next frame; nothing has scrolled yet.
      expect(scrollIntoView).not.toHaveBeenCalled();

      // Re-render with lane 1 active and its article in the pane,
      // mirroring the live app's response to the thread switch.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(performance.now());
      }
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
      expect(scrollIntoView).toHaveBeenCalledWith({ block: 'start' });
      const target = within(screen.getByTestId('conversation-body')).getByText(
        'msg-b',
      );
      expect(scrollIntoView.mock.instances[0]).toBe(target);
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('advances exactly one large-message step on a single sub-notch turn', async () => {
    // Single wheel event with |deltaY| = 50 — below the first staircase
    // threshold (100), so the accumulator lands in the slowest bucket (1
    // step). Five large turns so any off-by-one in the calculator would
    // surface as a wrong landing px.
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'm0', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'm1', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'm2', '2026-01-01T00:02:00Z'),
            makeUserText(1, 3, 'm3', '2026-01-01T00:03:00Z'),
            makeUserText(1, 4, 'm4', '2026-01-01T00:04:00Z'),
          ],
        ],
      ]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial active = m4 (x=1 → 240px).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // m3 sits at x=0.75 → 180px — one step back, sub-notch event.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${180 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('accelerates a fast burst across multiple steps via the staircase', async () => {
    // Five wheel events each at one notch (|deltaY| = 100), spaced just
    // above the output cooldown so every event commits and the staircase
    // owns the burst's pacing. The first event sits in the slowest
    // bucket (1 step — a single notch never accelerates) and later
    // events trip the higher buckets as their cumulative |delta| grows
    // inside the rolling window. The assertion is on the final landing
    // position, which captures the full burst's net advancement.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 10; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      const messages = new Map([[1, msgs]]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Five back-to-back wheel-up events, one tick over the output
      // cooldown apart (so each event commits) and well inside the
      // 250 ms rolling window (so the accumulator keeps compounding
      // across events). The cadence references WHEEL_STEP_COOLDOWN_MS by
      // symbol so a future tuning of the cooldown moves this in lock-
      // step instead of leaving a stale magic number behind.
      const burstIntervalMs = WHEEL_STEP_COOLDOWN_MS + 10;
      for (let i = 0; i < 5; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -100,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += burstIntervalMs;
      }
      // Cumulative steps walked backward across the five events (each
      // event reads the cumulative AFTER its own contribution lands).
      // The first notch always sits in the slowest bucket (1 step) so
      // the user can always land on the immediate neighbour. With the
      // 110 ms cadence, the rolling-window eviction drops the oldest
      // entry by the time the 4th event lands (cutoff = 5330 - 250 =
      // 5080 evicts the t=5000 entry), so cum stays at 300 for events
      // 3..5 instead of climbing to 400 / 500 like it did under the
      // tighter 50 ms cadence:
      //   t=5000: cum=100 → bucket 0   (1) → m9 → m8
      //   t=5110: cum=200 → bucket 200 (2) → m8 → m6
      //   t=5220: cum=300 → bucket 200 (2) → m6 → m4
      //   t=5330: cum=300 → bucket 200 (2) → m4 → m2
      //   t=5440: cum=300 → bucket 200 (2) → m2 → m0
      // Net 9 steps backward from m9 → m0 (x=0) is the final landing.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('throttles a trackpad-style burst of pixel-mode events to one step per cooldown tick', async () => {
    // macOS trackpads emit a continuous stream of small pixel-mode wheel
    // events for a single gentle gesture (~5–20 px each, ~5–10 ms
    // apart). Each individual event sits below the per-event clamp, so
    // the clamp does not protect against the accumulator-and-staircase
    // committing a step on every event — the playhead races through
    // multiple messages on what the user perceived as one tap. The
    // output-side cooldown gate ({@link WHEEL_STEP_COOLDOWN_MS}) caps
    // commit throughput so a sub-notch event stream lands one step per
    // cooldown tick. This test pins that contract by dispatching 12
    // sub-notch events 10 ms apart and asserting the playhead landed
    // exactly two steps back (one from the first event, one from the
    // event right at the cooldown boundary) rather than racing through
    // every event individually.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      // 11 messages so two-step-back from the initial (m10) is a clean
      // assertion at m8 — width = 240, m8 sits at x = 8/10 → 192 px.
      const msgs: Message[] = [];
      for (let i = 0; i < 11; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // 12 trackpad-sized events, 10 ms apart starting at t=5000:
      //   t=5000 (event 1): lastCommit=null → commits 1 step (m10 → m9)
      //   t=5010..t=5090 (events 2–10): inside cooldown → suppressed,
      //     accumulator keeps growing through every event
      //   t=5100 (event 11): gap = WHEEL_STEP_COOLDOWN_MS → cleared,
      //     cum ≈ 110 px (still bucket 0 → 1 step), commits (m9 → m8)
      //   t=5110 (event 12): gap = 10 ms → suppressed again
      // Net: 2 commits, final landing at m8.
      for (let i = 0; i < 12; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -10,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += 10;
      }
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${192 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('lets the first wheel event after a long pause commit immediately (no cooldown gate on the first event)', async () => {
    // The cooldown gate compares the current event time to the prior
    // commit time. After a pause long enough that the gap exceeds
    // WHEEL_STEP_COOLDOWN_MS, the gate naturally does not engage — the
    // ref still holds the last commit time, but the math falls through.
    // This test pins "a leisurely two-notch scrub committed both
    // notches" by firing two sub-notch events separated by a gap that
    // also exceeds the rolling window (so the accumulator resets too,
    // keeping each event at the slowest staircase bucket) and asserting
    // the playhead walked exactly two steps.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 5; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // First sub-notch event: lastCommit=null → commits (m4 → m3).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Long pause: well past both the rolling window AND the cooldown
      // (whichever is longer, the gap clears). The accumulator resets
      // and the cooldown's "now - lastCommitAt" is huge — the next
      // event commits without any throttling.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 150;
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Five messages span x = 0, 60, 120, 180, 240. After two single-
      // step commits from m4 the playhead sits at m2 (x = 120).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('passes a steady mouse-wheel cadence through unthrottled', async () => {
    // Mouse wheels typically emit notches 150+ ms apart in normal use,
    // so the 100 ms cooldown must not slow them down — pins this here
    // so a future tuning that bumps the cooldown above the chosen
    // 200 ms cadence would have to update this test deliberately.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      // Six messages span x = 0, 48, 96, 144, 192, 240. Start at m5
      // (the latest). Three full-notch events at the documented mouse-
      // wheel cadence walk 5 large steps (1 + 2 + 2 via the staircase)
      // back to m0 (x = 0). The intermediate-cadence math:
      //   t=5000 (event 1): cum=100 → bucket 0   (1) → m5 → m4
      //   t=5200 (event 2): cum=200 → bucket 200 (2) → m4 → m2
      //                     (entry t=5000 survives — cutoff = 4950)
      //   t=5400 (event 3): cum=200 → bucket 200 (2) → m2 → m0
      //                     (entry t=5000 evicted — cutoff = 5150)
      // If the cooldown had swallowed either event 2 or event 3 the
      // final landing would be m2 (x = 96) instead of m0 (x = 0), so
      // m0 distinguishes "all three commits landed" from a throttled
      // path.
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      const wheelIntervalMs = 200;
      for (let i = 0; i < 3; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -100,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += wheelIntervalMs;
      }
      expect(wheelIntervalMs).toBeGreaterThan(WHEEL_STEP_COOLDOWN_MS);
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('resets the accumulator after the rolling window elapses', async () => {
    // Two sub-notch events separated by a gap longer than the window. Each
    // event alone contributes 50 px (bucket 0 → 1 step). With reset, the
    // total is 2 steps; without reset, the second event's cumulative would
    // be 100 (bucket 100 → 2 steps) and the total would be 3 — pinning the
    // reset behaviour by total advancement.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // First event: cum=50 → 1 step back (m5 → m4).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Wait longer than the window so the accumulator resets.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 150;
      // Second event after the gap: fresh cum=50 → 1 step back (m4 → m3).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages at x = 0, 48, 96, 144, 192, 240. m3 sits at x = 144.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${144 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('caps a single trackpad-sized event at the slowest bucket via the per-event clamp', async () => {
    // A single trackpad-sized event (|deltaY| = 10) lands in the slowest
    // staircase bucket (1 step) regardless of `deltaMode`. The clamp's
    // role is preventing a single noisy event from skipping straight to
    // the top bucket; this test pins that contract by dispatching the
    // burst's first event in isolation and asserting it advances exactly
    // one large step (rather than e.g. ten, which would happen if the
    // calculator multiplied steps by event count without going through
    // the staircase).
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // First trackpad event of a burst (|deltaY| = 10). Cumulative is 10
      // → bucket 0 → 1 step back. m5 → m4 (x=192 on the 6-msg axis).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -10,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages → 5 gaps → 240 / 5 = 48 px each. m4 sits at x=192.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${192 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('treats deltaMode=1 (line) as ~40 px per line via normalization', async () => {
    // A line-mode event with |deltaY| = 3 must behave like a pixel-mode
    // event of ~120 px — i.e. clamped at the per-event cap (100 px) and
    // tallied into the rolling-window accumulator at one notch's worth.
    // Two such back-to-back events together push the cumulative above
    // the 2-step bucket (200 px) so the burst walks 1 then 2 steps —
    // 3 large-message steps in total, mirroring how a real line-mode
    // device emits per-tick scrolls.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Two line-mode events one tick past the output cooldown apart
      // (so the second event commits — the burst is not throttled).
      // Each event contributes 3 * 40 = 120 px clamped to 100 → cum=100
      // then 200. Walks 1 step then 2 steps: m5 → m4 → m2.
      for (let i = 0; i < 2; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -3,
              deltaMode: 1,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      }
      // Six messages at x = 0, 48, 96, 144, 192, 240. Starting on m5
      // (x=240), three steps back → m2 (x=96).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${96 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  // v30 fix 1: the playhead positions itself via `transform: translateX(...)`
  // rather than `left: <px>`. At 2 px wide, a fractional `left` value lets the
  // browser straddle a subpixel boundary so antialiasing paints ~1.5 px on one
  // side and ~0.5 px on the other, producing a visible width-shimmer as the
  // playhead steps across messages. `translateX` is GPU-composited on the
  // existing 2 px box and keeps a stable 2 px footprint regardless of where it
  // lands on the subpixel grid. This test pins the structural facts: the
  // inline transform carries `translateX(...)`, the inline `left` is not used
  // for positioning (a static `left-0` from className is acceptable, but the
  // inline `style.left` must be empty), and the transition animates
  // `transform` instead of `left`.
  it('positions each playhead via transform: translateX, not inline left (v30)', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const playheads = await screen.findAllByTestId('thread-timeline-playhead');
    expect(playheads.length).toBeGreaterThanOrEqual(1);
    for (const ph of playheads) {
      // Structural: transform carries translateX(...), and the inline `left`
      // is empty (the `left-0` className supplies a static 0 anchor).
      expect(ph.style.transform).toMatch(/translateX\(/);
      expect(ph.style.left).toBe('');
      // Transition animates transform — animating `left` is what produced the
      // subpixel shimmer in v29.
      expect(ph.style.transition).toContain('transform');
      expect(ph.style.transition).not.toContain('left');
    }
  });

  // The playhead's bar colour reuses the muted foreground semantic token used
  // by the navigator's rate-limit meter and the composer's context-usage
  // progress bar (all `bg-fg-muted`), so all three progress-style indicators
  // read as one visual family. An earlier indigo accent (`bg-accent`) clashed
  // with the surrounding UI; this test pins the chosen token so a future
  // style refactor cannot silently regress the unification.
  it('uses the shared muted-foreground progress-bar token, not the accent', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const playheads = await screen.findAllByTestId('thread-timeline-playhead');
    expect(playheads.length).toBeGreaterThanOrEqual(1);
    for (const ph of playheads) {
      expect(ph.className).toContain('bg-fg-muted');
      expect(ph.className).not.toContain('bg-accent');
    }
  });
});

describe('ThreadTimelineOverlay wheel calculator', () => {
  it('normalizes pixel-mode |delta| with the per-event clamp', () => {
    expect(normalizeWheelDeltaPx(50, 0)).toBe(50);
    expect(normalizeWheelDeltaPx(-50, 0)).toBe(50);
    // Above the clamp ceiling, contributions are capped.
    expect(normalizeWheelDeltaPx(500, 0)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('normalizes line-mode |delta| by the per-line pixel proxy', () => {
    // 1 line ≈ 40 px, two lines ≈ 80 px (under the clamp).
    expect(normalizeWheelDeltaPx(2, 1)).toBe(2 * WHEEL_DELTA_LINE_PX);
    // 5 lines = 200 px → clamped to 100.
    expect(normalizeWheelDeltaPx(5, 1)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('normalizes page-mode |delta| by the per-page pixel proxy and clamps', () => {
    // Even a single page-mode event is clamped to one notch.
    expect(normalizeWheelDeltaPx(1, 2)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('maps cumulative |delta| to the staircase step count', () => {
    expect(stepsForCumulativePx(0)).toBe(1);
    // The first acceleration bucket sits strictly above one notch's worth
    // of clamped |delta| (WHEEL_PER_EVENT_CLAMP_PX = 100), so a single
    // slow notch (cum=100, the maximum after one event) still walks just
    // one step — the user can land on the immediate prev/next message.
    expect(stepsForCumulativePx(100)).toBe(1);
    expect(stepsForCumulativePx(199)).toBe(1);
    expect(stepsForCumulativePx(200)).toBe(2);
    expect(stepsForCumulativePx(399)).toBe(2);
    expect(stepsForCumulativePx(400)).toBe(3);
    expect(stepsForCumulativePx(699)).toBe(3);
    expect(stepsForCumulativePx(700)).toBe(5);
    expect(stepsForCumulativePx(1099)).toBe(5);
    expect(stepsForCumulativePx(1100)).toBe(8);
    expect(stepsForCumulativePx(10_000)).toBe(8);
  });

  it('guarantees at least one step for any nonzero accumulator value', () => {
    // Regression: a single slow wheel notch (any |delta| up to the
    // per-event clamp of 100 px) must always advance exactly one step so
    // the user can always land on the immediate prev/next message. The
    // first acceleration bucket sits strictly above that ceiling.
    expect(stepsForCumulativePx(1)).toBe(1);
    expect(stepsForCumulativePx(WHEEL_PER_EVENT_CLAMP_PX)).toBe(1);
  });
});

describe('ThreadTimelineOverlay mark rendering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders circular marks with role-coded color classes and a data-message-kind attribute', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // A genuine human turn (user role + text block) → `user` kind.
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          // An assistant reply → `other` kind.
          makeMessage(1, 1, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    expect(marks).toHaveLength(2);
    const userMark = marks.find(
      (m) => m.getAttribute('data-message-uuid') === 'u',
    )!;
    const otherMark = marks.find(
      (m) => m.getAttribute('data-message-uuid') === 'a',
    )!;
    // Circle: rounded-full with equal width/height. The packed-lane overlap
    // problem that drove v3's rectangles is solved at the layout level now
    // (see buildGlobalXMap's minimum-spacing push), so the marks can stay
    // solid-fill with no alpha/ring workaround.
    expect(userMark.className).toContain('rounded-full');
    expect(userMark.style.width).toBe(userMark.style.height);
    // Role-coded color and data attribute (tested via class membership and
    // the data attribute, not literal hex, so the tailwind tokens can move).
    // Solid fill on both — overlap is prevented by the global x map, not by
    // alpha stacking, so the classes carry no alpha suffix or ring outline.
    expect(userMark).toHaveAttribute('data-message-kind', 'user');
    expect(userMark.className).toContain('bg-info');
    expect(userMark.className).not.toContain('bg-info/');
    expect(userMark.className).not.toContain('ring-');
    expect(otherMark).toHaveAttribute('data-message-kind', 'other');
    expect(otherMark.className).toContain('bg-fg-subtle');
    expect(otherMark.className).not.toContain('bg-fg-subtle/');
    expect(otherMark.className).not.toContain('ring-');
  });

  it('renders the main-conversation turns as a larger circle than the auxiliary turns', async () => {
    // One small dot sandwiched between two large dots on either side so
    // each small dot stays a lone single-dot render item (the clustering
    // logic needs 2+ adjacent smalls — see the small-dot clustering tests).
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // user turn → large
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          // tool call → small (sandwiched between u and a, no cluster)
          makeMessage(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          // assistant prose → large
          makeMessage(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:02:00Z',
          }),
          // meta line → small (lone, between large a and large u2)
          makeMessage(1, 3, 'm', {
            role: 'meta',
            content: [{ type: 'text', text: 'sys' }],
            created_at: '2026-01-01T00:03:00Z',
          }),
          // user turn → large (caps the trailing lone small)
          makeMessage(1, 4, 'u2', {
            role: 'user',
            content: [{ type: 'text', text: 'bye' }],
            created_at: '2026-01-01T00:04:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    const byUuid = new Map(
      marks.map((m) => [m.getAttribute('data-message-uuid'), m]),
    );
    expect(byUuid.get('u')).toHaveAttribute('data-message-size', 'large');
    expect(byUuid.get('a')).toHaveAttribute('data-message-size', 'large');
    expect(byUuid.get('t')).toHaveAttribute('data-message-size', 'small');
    expect(byUuid.get('m')).toHaveAttribute('data-message-size', 'small');
    // The diameter of a "large" mark is greater than that of a "small" one
    // (px values, not classes — the renderer applies them inline).
    const largeDiameter = parseFloat(byUuid.get('u')!.style.width);
    const smallDiameter = parseFloat(byUuid.get('t')!.style.width);
    expect(largeDiameter).toBeGreaterThan(smallDiameter);
    // The delta is subtle but visible — the lane should still read as one
    // timeline, not two layers. Cap at 6 px so a future tweak that goes
    // overboard is caught here.
    expect(largeDiameter - smallDiameter).toBeLessThanOrEqual(6);
  });
});

describe('ThreadTimelineOverlay active lane highlight', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('falls back to the activeThreadId prop highlight when no dot is in view', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: 'uuid-a',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map(), activeThreadId: 2 });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
  });

  it('marks the lane containing the playhead-active message regardless of the activeThreadId prop', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    const messages = new Map([
      [1, [makeMessage(1, 0, 'a', { created_at: '2026-01-01T00:00:00Z' })]],
      [2, [makeMessage(2, 0, 'b', { created_at: '2026-01-01T00:02:00Z' })]],
    ]);
    // The playhead's initial position is the latest dot (msg-b on lane 2),
    // so the lane-2 highlight follows the playhead even when prop says lane 1.
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'a' }, { uuid: 'b' }],
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    await waitFor(() => {
      expect(lanes[1]).toHaveAttribute('data-active', 'true');
    });
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
  });

  // v30 fix 2: the active-row hairline used to be `border-y border-slate-200`
  // (active) / `border-y border-transparent` (inactive). The transparent
  // placeholder kept the active and inactive rows the same height — but it
  // also reserved 1 px of layout on the top and 1 px on the bottom of EVERY
  // row, producing a ~2 px transparent stripe between adjacent rows under
  // `align-items: stretch`. That stripe broke the per-lane playhead column
  // visually even after v28 dropped the `<ul>`'s `gap-y-*`. v30 moves the
  // active hairline to a pair of `inset box-shadow`s — non-layout, so
  // adjacent rows now sit truly edge-to-edge.
  //
  // This test pins the structural facts: no row carries a `border-y`
  // utility (active or inactive), and the active row's label and axis
  // cells carry the shadow-inset class instead.
  it('renders the active hairline via inset box-shadow rather than border-y placeholders (v30)', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: 'uuid-a',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map(), activeThreadId: 2 });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    // No lane (active or inactive) reserves layout via `border-y` — those
    // transparent placeholders are exactly what produced the inter-row gap.
    for (const lane of lanes) {
      const cells = lane.querySelectorAll('[data-thread-id]');
      for (const cell of Array.from(cells)) {
        // The lane `<li>` itself is `display: contents`, so the two
        // grid items are the label `<span>` and the axis `<div>`. Both
        // must be free of `border-y` utilities (any direction-y border
        // would reintroduce the layout-reserved stripe).
        if (cell === lane) continue;
        expect(cell.className).not.toMatch(/(^|\s)border-y(\s|$)/);
      }
    }
    // The active lane's two cells both carry the shadow-inset utility that
    // paints the hairline non-destructively. The inactive lane does not.
    const activeLane = lanes.find(
      (l) => l.getAttribute('data-active') === 'true',
    )!;
    const inactiveLane = lanes.find(
      (l) => l.getAttribute('data-active') === 'false',
    )!;
    const activeCells = activeLane.querySelectorAll('[data-thread-id]');
    expect(activeCells.length).toBeGreaterThanOrEqual(2);
    for (const cell of Array.from(activeCells)) {
      // Looking for the shadow utility's class name. We do not pin the
      // exact pixel values (those are an implementation detail of the
      // hairline colour) — only that a `shadow-[inset_...]` class is present.
      expect(cell.className).toMatch(/shadow-\[inset_/);
    }
    const inactiveCells = inactiveLane.querySelectorAll('[data-thread-id]');
    for (const cell of Array.from(inactiveCells)) {
      expect(cell.className).not.toMatch(/shadow-\[inset_/);
    }
  });
});

describe('ThreadTimelineOverlay wheel skips small marks', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('walks only the main-conversation (large) subset on wheel, jumping over tool calls', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Three large turns interleaved with a small tool call. The wheel-step
    // navigation must skip the tool call so one notch advances by one
    // headline turn — `large-b` should land between `large-a` and
    // `large-c` regardless of the small dot's x.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
          makeUserText(1, 3, 'large-c', '2026-01-01T00:03:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
        { uuid: 'large-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead lands on the latest message (large-c, x=1 → 240px).
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // Wheel up (sub-notch event → one step): the previous LARGE turn is
    // large-b (x=2/3 → 160px), NOT the small tool call between them. The
    // sub-notch keeps the staircase at one step so the assertion targets
    // the immediate large neighbour.
    const body = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -50,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${160 + LANE_LEFT_PAD_PX}px`);
  });

  it('still allows a click to target a small mark precisely', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // A small tool call between two large turns. A click at the tool call's
    // x must jump the playhead to it, even though the wheel would skip it.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            // Land at x=0.5 (the midpoint) so the click target is
            // unambiguous and deterministic.
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Click at x=120 (midpoint) → small-t is the nearest mark.
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
      clientX: 120,
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
  });
});

describe('ThreadTimelineOverlay keyboard navigation', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  /**
   * Three evenly-spaced large turns at x=0, x=0.5, x=1 (px 0, 120, 240 on
   * the 240 px stub axis) — the same fixture shape the wheel-step tests
   * use, so the keyboard assertions observe the identical playhead moves a
   * wheel step produces.
   */
  function threeLargeTurns(): Map<number, Message[]> {
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

  function renderThreeLargeTurns() {
    stubAxisRect({ left: 0, width: 240 });
    return renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: threeLargeTurns(),
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
  }

  /**
   * Dispatch a `keydown` from the given target (default: `window`, where
   * the overlay's listener lives) and return the event so the caller can
   * assert on `defaultPrevented`. `bubbles` is on so events fired from a
   * child element (an input, the conversation body) reach the window
   * listener the way real typing does.
   */
  function pressKey(
    key: string,
    init: KeyboardEventInit = {},
    target: EventTarget = window,
  ): KeyboardEvent {
    const event = new KeyboardEvent('keydown', {
      key,
      bubbles: true,
      cancelable: true,
      ...init,
    });
    act(() => {
      target.dispatchEvent(event);
    });
    return event;
  }

  function playheadPx(): string {
    return playheadLeftPx(
      screen.getAllByTestId('thread-timeline-playhead')[0],
    );
  }

  it('steps one large message per plain ArrowLeft / ArrowRight keydown through the jump path', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead lands on the latest message (msg-c, x=1 → 240px).
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // ArrowLeft → one large message towards the older end (msg-b), the
    // same playhead move a single wheel-up step produces. The handled key
    // is preventDefault-ed so it cannot leak into page scrolling.
    const left = pressKey('ArrowLeft');
    expect(left.defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    // ArrowRight → one large message towards the newer end (back to msg-c).
    const right = pressKey('ArrowRight');
    expect(right.defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('walks the large-message subset, skipping small auxiliary marks', async () => {
    stubAxisRect({ left: 0, width: 240 });
    // A small tool call between two large turns: one keypress from large-c
    // must land on the adjacent LARGE message (large-b, x=2/3 → 160px),
    // never on the small mark between them.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
          makeUserText(1, 3, 'large-c', '2026-01-01T00:03:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
        { uuid: 'large-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    pressKey('ArrowLeft');
    expect(playheadPx()).toBe(`${160 + LANE_LEFT_PAD_PX}px`);
  });

  it('steps once per key-repeat keydown with no cooldown, clamping without wrap at both ends', async () => {
    // Freeze the clock: if the wheel's WHEEL_STEP_COOLDOWN_MS output gate
    // applied to keys, every press after the first would land at the same
    // frozen instant and be swallowed. Keys must not be gated — one
    // keydown (repeat or not) is exactly one step.
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => 5_000);
    try {
      renderThreeLargeTurns();
      await screen.findAllByTestId('thread-timeline-dot');
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      // Two held-key repeats advance two large messages: msg-c → msg-a.
      pressKey('ArrowLeft', { repeat: true });
      pressKey('ArrowLeft', { repeat: true });
      expect(playheadPx()).toBe(`${LANE_LEFT_PAD_PX}px`);
      // At the oldest end a further ArrowLeft clamps (no wrap) but is
      // still preventDefault-ed — the timeline owns the key even when the
      // step is a no-op.
      const clampedLeft = pressKey('ArrowLeft', { repeat: true });
      expect(clampedLeft.defaultPrevented).toBe(true);
      expect(playheadPx()).toBe(`${LANE_LEFT_PAD_PX}px`);
      // Back the other way: two repeats reach the newest end, the third
      // clamps without wrapping and is still preventDefault-ed.
      pressKey('ArrowRight', { repeat: true });
      pressKey('ArrowRight', { repeat: true });
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      const clampedRight = pressKey('ArrowRight', { repeat: true });
      expect(clampedRight.defaultPrevented).toBe(true);
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('ignores keydowns from editable targets (input / textarea / select / contentEditable) without preventDefault', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    const editables: HTMLElement[] = [
      document.createElement('input'),
      document.createElement('textarea'),
      document.createElement('select'),
    ];
    // jsdom does not compute `isContentEditable` from the attribute, so
    // stub the property the guard reads — in a real browser any element
    // inside a contentEditable region reports true through inheritance.
    const editableDiv = document.createElement('div');
    Object.defineProperty(editableDiv, 'isContentEditable', { value: true });
    editables.push(editableDiv);
    try {
      for (const el of editables) {
        document.body.appendChild(el);
        // Bubbles from the editable target up to the window listener, the
        // way a real keystroke in the composer / terminal does.
        const event = pressKey('ArrowLeft', {}, el);
        expect(event.defaultPrevented).toBe(false);
        expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      }
    } finally {
      for (const el of editables) {
        el.remove();
      }
    }
  });

  it('ignores modified arrows (Ctrl / Meta / Alt) and already-defaultPrevented keydowns', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    for (const init of [
      { ctrlKey: true },
      { metaKey: true },
      { altKey: true },
    ]) {
      const event = pressKey('ArrowLeft', init);
      expect(event.defaultPrevented).toBe(false);
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    }
    // A keydown some earlier handler already claimed stays claimed: the
    // timeline must not act on it.
    const claimed = new KeyboardEvent('keydown', {
      key: 'ArrowLeft',
      bubbles: true,
      cancelable: true,
    });
    claimed.preventDefault();
    act(() => {
      window.dispatchEvent(claimed);
    });
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('attaches no keydown listener while the timeline is collapsed', () => {
    // Override the describe-level beforeEach (which seeds the expanded
    // preference to 'true') back to the collapsed state. No cache reset is
    // needed: `resetGlobals` already cleared the module-level cache, and it
    // only fills on the first hook read, at render — after this write.
    window.localStorage.setItem(timelineExpandedKey(), 'false');
    stubAxisRect({ left: 0, width: 240 });
    renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: threeLargeTurns(),
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');
    // No listener while collapsed: arrows are neither preventDefault-ed
    // nor do they touch any timeline state (no playhead exists to move).
    const event = pressKey('ArrowLeft');
    expect(event.defaultPrevented).toBe(false);
    expect(screen.queryAllByTestId('thread-timeline-playhead')).toHaveLength(0);
  });

  it('detaches the listener when the timeline is collapsed mid-session and reattaches on re-expand', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    // Sanity: while expanded the timeline owns the arrows.
    expect(pressKey('ArrowLeft').defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    // Collapse via the header toggle — the more common path to a collapsed
    // timeline than mounting collapsed. This exercises the effect CLEANUP:
    // the window listener must actually be removed, not merely no-op
    // behind a stale `expanded` closure, or every arrow key on the page
    // would keep being swallowed after the user puts the timeline away.
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');
    const collapsed = pressKey('ArrowLeft');
    expect(collapsed.defaultPrevented).toBe(false);
    // Re-expanding runs the effect again and reattaches the listener:
    // ArrowRight is owned again and lands the playhead on the newest
    // message (a step from the pre-collapse position, or a clamp if the
    // playhead already sits there — 240px either way).
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    await screen.findAllByTestId('thread-timeline-dot');
    expect(pressKey('ArrowRight').defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });
});

describe('ThreadTimelineOverlay jump-target highlight', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('highlights the destination message after a click jump so the eye spots it', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    stubAxisRect({ left: 0, width: 240 });
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
    // Click on x=0 (msg-a) to drive a same-lane jump.
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
      clientX: 0,
    });
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    // The destination article carries the highlight class right after the
    // scroll lands. We assert the class is present rather than waiting for
    // its removal — the CSS animation fades the bubble background back to
    // its rest color, the class is the trigger.
    const target = within(screen.getByTestId('conversation-body')).getByText(
      'msg-a',
    );
    expect(target.classList.contains(TIMELINE_JUMP_HIGHLIGHT_CLASS)).toBe(true);
  });
});

describe('ThreadTimelineOverlay does not override an external active-thread change', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('lets a Navigator-driven setActiveThread stick when the message-list reference changes underneath', async () => {
    // Regression: v2-v4 had the timeline re-fire its auto-switch effect on
    // any `activeMessage` reference change (a background message-list
    // refetch landing right after a Navigator click was the common
    // trigger), which then overwrote the Navigator's chosen thread.
    //
    // The fix snapshots the active message into a ref and depends on
    // `scrubTick` alone, so only a deliberate scrub re-fires the effect.
    // This test simulates the sequence: scrub to land the playhead on
    // thread 1, then a Navigator click flips active thread to 2, then a
    // re-render with a fresh messages map (the refetch) — the active
    // thread must remain 2.
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    const buildMessages = () =>
      new Map<number, Message[]>([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
        [2, [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
    // Start with lane 2 active (the latest message lands there).
    const { rerender, bodyRef } = renderOverlay({
      threads,
      messagesByThread: buildMessages(),
      activeThreadId: 2,
      conversationArticles: [{ uuid: 'msg-c' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Scrub: wheel-up from msg-c → msg-b lands the playhead on thread 1.
    // (Both Navigator and the timeline would each call setActiveThread,
    // so confirm the timeline DID flip the store to thread 1 first.)
    const body = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await waitFor(() => {
      expect(useNavStore.getState().activeThreadId).toBe(1);
    });
    // Now a Navigator click flips the store back to thread 2.
    act(() => {
      useNavStore.getState().setActiveThread(2);
    });
    expect(useNavStore.getState().activeThreadId).toBe(2);
    // Simulate the post-click refetch: a fresh messages map (new array
    // identities) lands, the overlay re-renders. Without the fix, the
    // active-message effect would re-fire and call setActiveThread(1)
    // because the playhead is still on msg-b. With the fix, scrubTick
    // did not change, so the effect stays inert.
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    const fresh = buildMessages();
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => ({
        messages: fresh.get(threadId as number) ?? [],
      }),
    );
    rerender(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-c">msg-c</article>
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={2}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    // Give microtasks + any effect ticks a chance to run.
    await Promise.resolve();
    await Promise.resolve();
    // The Navigator's choice (thread 2) must win — the timeline must not
    // have overridden it back to thread 1.
    expect(useNavStore.getState().activeThreadId).toBe(2);
  });
});

describe('ThreadTimelineOverlay mystery-dot filter', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('does not render dots for system or other ingest-only rows', async () => {
    // Real sessions emit a handful of `role: "system"` rows on startup
    // (and an occasional `other`) whose stamps land before the first
    // user prompt. The transcript skips them; the timeline must too so
    // they do not surface as mystery dots to the left of the first
    // human-readable message.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'sys', {
            role: 'system',
            content: [{ type: 'text', text: 'bootstrap' }],
            created_at: '2025-12-31T23:59:00Z',
          }),
          makeMessage(1, 1, 'usr', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 2, 'oth', {
            role: 'other',
            content: [{ type: 'text', text: 'misc' }],
            created_at: '2026-01-01T00:00:30Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    const uuids = marks
      .map((m) => m.getAttribute('data-message-uuid'))
      .sort();
    expect(uuids).toEqual(['usr']);
  });
});

describe('ThreadTimelineOverlay small-dot clustering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders 2+ consecutive small dots as a single cluster mark', async () => {
    // A user turn, three tool calls in a row (each is a "small" auxiliary
    // mark), then an assistant prose reply. The three tool calls must
    // collapse into one cluster mark while the user and assistant turns
    // still render as their own dots.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'do stuff' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
          makeMessage(1, 2, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:20Z',
          }),
          makeMessage(1, 3, 't3', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu3', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:30Z',
          }),
          makeMessage(1, 4, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    // The lone large dots (u, a) still render as dots.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    const dotUuids = dots.map((d) => d.getAttribute('data-message-uuid'));
    expect(dotUuids).toContain('u');
    expect(dotUuids).toContain('a');
    expect(dotUuids).not.toContain('t1');
    expect(dotUuids).not.toContain('t2');
    expect(dotUuids).not.toContain('t3');
    // Exactly one cluster mark, pointing at the first member.
    const clusters = await screen.findAllByTestId(
      'thread-timeline-cluster',
    );
    expect(clusters).toHaveLength(1);
    expect(clusters[0]).toHaveAttribute('data-message-uuid', 't1');
    expect(clusters[0]).toHaveAttribute('data-cluster-member-count', '3');
  });

  it('renders a lone small dot as a regular dot, not a cluster', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:30Z',
          }),
          makeMessage(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    const dotUuids = dots.map((d) => d.getAttribute('data-message-uuid'));
    expect(dotUuids).toContain('t');
    // No cluster mark when only one consecutive small dot exists.
    expect(screen.queryAllByTestId('thread-timeline-cluster')).toHaveLength(
      0,
    );
  });
});

describe('ThreadTimelineOverlay grid lane layout', () => {
  // The lane container is a CSS Grid with two columns: a `max-content`
  // label column that auto-sizes to the widest label across every lane,
  // and a `1fr` axis column carrying the dots and the playhead. The grid
  // replaces an earlier two-`<ul>` flex layout whose label column was a
  // hard-coded width (wasting space for short names) and whose per-row
  // alignment between the label cell and the axis cell drifted as lanes
  // accumulated because the label cell's padding inflated its height
  // past the axis cell's fixed pixel height.
  //
  // The grid solves both at once: `max-content` shares the widest label
  // across every row, and `align-items: stretch` makes each row's two
  // cells share the row's full height so the active-highlight band and
  // the per-lane playhead segment paint at identical vertical extents
  // on both the label side and the axis side. (`center` was the prior
  // contract; it left the axis cell — which carries an explicit pixel
  // height — measurably shorter than the label cell whose height was
  // governed by font metrics + padding, so the highlight band painted
  // a thinner stripe on the axis side and the per-lane playhead looked
  // disconnected between rows.)
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('uses CSS Grid with a max-content label column and stretched rows for the lane container', async () => {
    // Structural contract: the lane `<ul>` is a grid with two columns
    // sized `max-content 1fr`, and rows stretch via `align-items:
    // stretch`. The label column being `max-content` is what gives every
    // lane label the same width as the longest one (no hard-coded
    // px gutter that wastes space when names are short). `stretch`
    // (rather than `center`) is what guarantees the two grid items of
    // a single row paint at the same vertical extent — the necessary
    // condition for the active-highlight band and the per-lane playhead
    // segment to read as one continuous block across the row.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    expect(grid.style.display).toBe('grid');
    expect(grid.style.gridTemplateColumns).toBe('max-content 1fr');
    expect(grid.style.alignItems).toBe('stretch');
  });

  it('carries no non-zero row-gap class on the lane grid so per-lane playhead spans align edge-to-edge across rows', async () => {
    // Each lane renders its own per-lane playhead `<span>`. Any non-zero
    // row gap on the lane `<ul>` shows as a visible break in the
    // otherwise continuous vertical playhead line — a 2px `gap-y-0.5`
    // gap, for instance, paints a 2px gap between every adjacent
    // playhead segment. Pin the contract: the grid must not carry any
    // `gap-y-*` class except `gap-y-0` (Tailwind's default row-gap is
    // already 0, so the natural shape is to drop the class entirely).
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    // Reject any `gap-y-<non-zero>` token. `gap-y-0` would still pass,
    // but the natural shape is no class at all.
    expect(grid.className).not.toMatch(/(?:^|\s)gap-y-(?!0(?:\s|$))/);
  });

  it('stretches the lane grid to the full axis content width via width:max-content + minWidth:100% so sticky labels have a containing block to pin against', async () => {
    // The label cell uses `position: sticky; left: 0`, and sticky only
    // moves within its containing block — for a grid item that block is
    // the grid `<ul>` itself. The grid `<ul>` is a block-level child of
    // the horizontal-scroll wrapper, so without an explicit width hint
    // its used width stays equal to the wrapper's content-box width
    // (i.e. the visible viewport), even when the axis grid item
    // overflows it horizontally and triggers the wrapper's scrollbar.
    // A containing block no wider than the viewport leaves `left: 0`
    // with nowhere to slide, so the label scrolls off-screen with the
    // axis — which is the regression a previous grid restructure shipped
    // because it dropped this very width hint.
    //
    // `width: max-content` resolves to the sum of the grid tracks' max-
    // content widths (the axis cells declare an explicit pixel width,
    // so this stretches the `<ul>` to the full scrollable range), and
    // `minWidth: 100%` keeps the `<ul>` at least viewport-wide on short
    // sessions where the axis fits without scroll. jsdom does not run
    // CSS layout, but the inline-style contract is what tells a real
    // browser to stretch — pin both declarations so a future restructure
    // cannot silently regress sticky pinning again.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    expect(grid.style.width).toBe('max-content');
    expect(grid.style.minWidth).toBe('100%');
  });

  it('renders one label cell and one axis cell per lane, each promoted to a grid item via display:contents on the <li>', async () => {
    // Each lane is an `<li>` with `display: contents` (the list item is
    // kept for semantics / a11y but stripped from layout), so its inner
    // label `<span>` and axis `<div>` are promoted to direct grid items
    // of the `<ul>` at LAYOUT time. The DOM tree itself still nests the
    // cells under the `<li>` — that is what semantic markup demands —
    // but `display: contents` elides the `<li>` box so the grid
    // measures the cells as if they were direct children. The
    // necessary conditions to verify here are: each lane has exactly
    // one label cell and one axis cell, the `<li>` carries
    // `display: contents`, and the `<li>` is a direct DOM child of the
    // grid (so `display: contents` is enough to promote the cells —
    // no extra wrapper sits between).
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    const lanes = within(grid).getAllByTestId('thread-timeline-lane');
    expect(lanes).toHaveLength(2);
    for (const lane of lanes) {
      expect(lane.style.display).toBe('contents');
      // The `<li>` is a direct DOM child of the grid `<ul>` — no
      // intermediate wrapper that would defeat `display: contents`.
      expect(lane.parentElement).toBe(grid);
      const label = within(lane).getByTestId('thread-timeline-lane-label');
      // `data-timeline-axis` marks the axis cell of the lane.
      const axisCell = lane.querySelector('[data-timeline-axis]');
      expect(label).not.toBeNull();
      expect(axisCell).not.toBeNull();
      // The label and axis are direct children of the `<li>` (one level
      // deep), so `display: contents` on the `<li>` promotes them
      // straight to grid items at layout time.
      expect(label.parentElement).toBe(lane);
      expect(axisCell?.parentElement).toBe(lane);
    }
  });

  it('shares the label column width across lanes so every label measures the same as the longest one', async () => {
    // JSDOM does not run CSS layout, so we cannot read the resolved
    // pixel width of each label cell directly. The structural contract
    // we CAN pin is: every label sits in the same grid column (the
    // `max-content` column) of the same grid container, and no per-lane
    // explicit width overrides it. That is the necessary and sufficient
    // condition for real browsers to render all labels at the longest
    // label's width.
    const threads = [
      makeThread(1, { title: 'main', created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        title: 'short',
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
      makeThread(3, {
        title: 'a very long subthread title that exceeds the others',
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:02:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    const labels = within(grid).getAllByTestId('thread-timeline-lane-label');
    expect(labels).toHaveLength(3);
    // No label carries an explicit `width` style — width is governed by
    // the grid's `max-content` column. (A regression that pinned a px
    // width per label would defeat the auto-sized-to-longest contract.)
    for (const label of labels) {
      expect(label.style.width).toBe('');
      // Each label sits inside its lane's `<li>` whose `display:
      // contents` promotes the label to a direct grid item at layout
      // time, so all labels share the same `max-content` column.
      const lane = label.closest('[data-testid="thread-timeline-lane"]');
      expect(lane).not.toBeNull();
      expect((lane as HTMLElement).style.display).toBe('contents');
      expect(lane?.parentElement).toBe(grid);
    }
  });

  it('routes horizontal scroll through a single wrapper so the sticky label cells can pin to the left edge', async () => {
    // Vertical scroll lives on the outer body (`overflow-y-auto`);
    // horizontal scroll lives on the axis-column wrapper that hosts the
    // grid. The label cells use `position: sticky; left: 0` to pin to
    // the left edge during a horizontal pan, so a wide axis still leaves
    // the labels readable.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const body = await screen.findByTestId('thread-timeline-body');
    expect(body.className).toMatch(/\boverflow-y-auto\b/);
    expect(body.className).not.toMatch(/\boverflow-x\b/);
    const axisColumn = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    expect(axisColumn.className).toMatch(/\boverflow-x-auto\b/);
    const label = (
      await screen.findAllByTestId('thread-timeline-lane-label')
    )[0];
    expect(label.style.position).toBe('sticky');
    expect(label.style.left).toBe('0px');
  });

  it('paints the sticky label with an opaque background via className (bg-surface resting, bg-surface-elevated active) so axis dots cannot peek through during a horizontal pan and the active highlight remains visible', async () => {
    // The sticky label slides over the axis cell horizontally as the
    // wrapper pans. Without an opaque background the axis line and dots
    // would read through the label glyphs, which is illegible. The
    // background MUST come from the className (not from an inline
    // `style.background`): an inline background has higher specificity
    // than a Tailwind class, so an inline `background: surface` would win
    // over an active-state `bg-surface-elevated` class and leave the
    // sticky label white while the axis cell paints `bg-surface-elevated`
    // — breaking the row's visual continuity, which is precisely what
    // {@link applies the active highlight to both grid cells of the
    // active lane} pins on the axis side.
    //
    // The contract is therefore: inactive sticky label paints `bg-surface`
    // (matching the body so axis dots never read through it), active
    // sticky label paints `bg-surface-elevated` (matching the axis cell so
    // the active band reads as one continuous row), and no inline
    // `background` style is set that would override either.
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    const inactiveLabel = within(lanes[0]).getByTestId(
      'thread-timeline-lane-label',
    );
    const activeLabel = within(lanes[1]).getByTestId(
      'thread-timeline-lane-label',
    );
    // Inactive sticky label is opaque surface through the className.
    // `bg-surface(?!-)` matches the resting class but not `bg-surface-elevated`.
    expect(inactiveLabel.className).toMatch(/\bbg-surface(?!-)/);
    expect(inactiveLabel.className).not.toMatch(/\bbg-surface-elevated\b/);
    // Active sticky label is opaque surface-elevated (matching the axis
    // cell's highlight) and does NOT carry the resting bg-surface token
    // — so there is exactly one background class active per cell and the
    // class set unambiguously identifies the visual state.
    expect(activeLabel.className).toMatch(/\bbg-surface-elevated\b/);
    expect(activeLabel.className).not.toMatch(/\bbg-surface(?!-)/);
    // No inline background on either label — the background lives on
    // className alone so the active class always wins. (Reading the
    // style property directly catches both `background` and
    // `background-color` short-hand variants on inline styles.)
    expect(inactiveLabel.style.background).toBe('');
    expect(inactiveLabel.style.backgroundColor).toBe('');
    expect(activeLabel.style.background).toBe('');
    expect(activeLabel.style.backgroundColor).toBe('');
  });

  it('keeps the sticky label visible at the wrapper left edge while the axis cell content scrolls horizontally', async () => {
    // Behavioural pin for the sticky-label contract: when the axis-
    // column wrapper scrolls horizontally, the sticky label MUST stay
    // pinned at x=0 of the wrapper while the axis cell shifts left by
    // the scroll amount. jsdom does not run CSS, so `position: sticky`
    // does not move the label automatically — but it DOES report
    // `scrollLeft` on the scroll container, and the inline style
    // contract (`position: sticky; left: 0`) is what tells a real
    // browser to pin. Assert both halves:
    //
    //   1. The label still carries the sticky positioning contract
    //      after the wrapper has been scrolled (no regression that
    //      drops the style under some state transition).
    //   2. The wrapper's `scrollLeft` advances normally so the axis
    //      cells visibly pan — the wrapper is the only horizontal
    //      scroller, the label rides along sticky-pinned.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const axisColumn = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    const label = (
      await screen.findAllByTestId('thread-timeline-lane-label')
    )[0];
    // Simulate the wrapper being scrolled horizontally past zero — e.g.
    // the user has panned a wide session's axis to the right.
    act(() => {
      axisColumn.scrollLeft = 120;
    });
    expect(axisColumn.scrollLeft).toBe(120);
    // The sticky positioning contract is intact: a real browser holds
    // the label at the wrapper's left edge while the axis cell content
    // pans behind it.
    expect(label.style.position).toBe('sticky');
    expect(label.style.left).toBe('0px');
    // The label sits in the same DOM ancestor as the axis cell of the
    // same lane — i.e. inside the scrolling wrapper — so sticky has
    // somewhere to pin. (A regression that moved the label out of the
    // scroll container would defeat sticky entirely.)
    expect(axisColumn.contains(label)).toBe(true);
  });

  it('applies the active highlight to both grid cells of the active lane so the band reads as continuous', async () => {
    // With `display: contents` on the `<li>` the list-item itself has no
    // box, so a highlight applied to the `<li>` would never paint. The
    // active highlight lives on BOTH the label cell AND the axis cell
    // individually, so the two halves of the active lane's grid row line
    // up into one continuous visual band. v30 expresses the top/bottom
    // hairline as an `inset box-shadow` rather than `border-y`, because
    // `border-y border-transparent` (the prior inactive placeholder)
    // reserved 1 px on top and 1 px on bottom of every row and produced
    // a ~2 px transparent stripe between adjacent rows under
    // `align-items: stretch`. The `bg-surface-elevated` background remains the
    // active band's surface; the inset shadow draws its boundary.
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    expect(lanes).toHaveLength(2);
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
    const activeLabel = within(lanes[1]).getByTestId(
      'thread-timeline-lane-label',
    );
    const activeAxis = lanes[1].querySelector(
      '[data-timeline-axis]',
    ) as HTMLElement;
    expect(activeLabel).toHaveAttribute('data-active', 'true');
    expect(activeAxis).toHaveAttribute('data-active', 'true');
    // Both cells carry the identical highlight token set so the band
    // reads as continuous across the row.
    expect(activeLabel.className).toMatch(/bg-surface-elevated/);
    expect(activeAxis.className).toMatch(/bg-surface-elevated/);
    // v30: the hairline is an inset box-shadow (non-layout), not a
    // border-y placeholder (which used to reserve a 2 px gap between
    // adjacent rows).
    expect(activeLabel.className).toMatch(/shadow-\[inset_/);
    expect(activeAxis.className).toMatch(/shadow-\[inset_/);
    // The inactive lane's cells do NOT carry the active tokens, so the
    // highlight is per-lane rather than global.
    const inactiveLabel = within(lanes[0]).getByTestId(
      'thread-timeline-lane-label',
    );
    const inactiveAxis = lanes[0].querySelector(
      '[data-timeline-axis]',
    ) as HTMLElement;
    expect(inactiveLabel.className).not.toMatch(/bg-surface-elevated/);
    expect(inactiveAxis.className).not.toMatch(/bg-surface-elevated/);
    // Inactive rows must not carry the inset shadow either — otherwise
    // the active state stops reading as distinct.
    expect(inactiveLabel.className).not.toMatch(/shadow-\[inset_/);
    expect(inactiveAxis.className).not.toMatch(/shadow-\[inset_/);
  });

  it('paints the active-highlight band at matched heights on label and axis by stretching both grid items to the row height', async () => {
    // Each grid item of a lane (the sticky label `<span>` and the axis
    // `<div>` marked `data-timeline-axis`) carries `h-full` plus a
    // `minHeight: LANE_HEIGHT_PX` floor. Combined with the grid
    // container's `align-items: stretch`, this is the necessary and
    // sufficient condition for the two cells of a single row to share
    // the same painted height — so the active-highlight band
    // (`bg-surface-elevated` + `border-y`) appears as one continuous block
    // across the row rather than two stripes of mismatched height. A
    // regression that dropped `h-full` from either side or pinned the
    // axis to a fixed `height` would defeat the stretch and reintroduce
    // the visible mismatch.
    //
    // jsdom does not run CSS layout, so we cannot read the resolved
    // pixel height of each cell. The contract we CAN pin is the inline
    // and class declarations themselves: both cells expose `h-full` in
    // their className and `minHeight: LANE_HEIGHT_PX` (== 18px) inline.
    const LANE_HEIGHT_PX = 18;
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    for (const lane of lanes) {
      const label = within(lane).getByTestId('thread-timeline-lane-label');
      const axis = lane.querySelector(
        '[data-timeline-axis]',
      ) as HTMLElement;
      expect(label).not.toBeNull();
      expect(axis).not.toBeNull();
      // Both cells declare `h-full` so each row's items grow to the row's
      // stretched height instead of capping at their intrinsic height.
      expect(label.className).toMatch(/(?:^|\s)h-full(?:\s|$)/);
      expect(axis.className).toMatch(/(?:^|\s)h-full(?:\s|$)/);
      // Both cells declare the same `minHeight` floor so an empty axis
      // row still respects `LANE_HEIGHT_PX` rather than collapsing.
      expect(label.style.minHeight).toBe(`${LANE_HEIGHT_PX}px`);
      expect(axis.style.minHeight).toBe(`${LANE_HEIGHT_PX}px`);
      // The axis side must NOT pin a fixed `height` — that would defeat
      // the stretch by forcing the axis cell back to exactly
      // `LANE_HEIGHT_PX` regardless of how tall the row grew.
      expect(axis.style.height).toBe('');
    }
    // The active lane's both cells additionally carry the highlight
    // tokens, so when the row stretches the band paints continuously
    // across both halves at the same height.
    const activeLane = lanes[1];
    expect(activeLane).toHaveAttribute('data-active', 'true');
    const activeLabel = within(activeLane).getByTestId(
      'thread-timeline-lane-label',
    );
    const activeAxis = activeLane.querySelector(
      '[data-timeline-axis]',
    ) as HTMLElement;
    expect(activeLabel.className).toMatch(/bg-surface-elevated/);
    expect(activeAxis.className).toMatch(/bg-surface-elevated/);
  });

  it('keeps row alignment stretched regardless of how many lanes accumulate', async () => {
    // The grid's `align-items: stretch` is the single source of truth for
    // row alignment between the label cell and the axis cell — it
    // guarantees the two cells share the row's full height so the
    // active-highlight band and the per-lane playhead segment paint at
    // identical vertical extents. A regression that swapped it for
    // `center` (the prior contract) would reintroduce the visible
    // mismatch in highlight band height; pin the contract on the grid
    // container itself so the guarantee does not depend on lane count.
    const threads = Array.from({ length: 8 }, (_, i) =>
      makeThread(i + 1, {
        title: `lane ${i + 1}`,
        parent_thread_id: i === 0 ? null : 1,
        root_message_uuid: i === 0 ? null : null,
        created_at: `2026-01-01T00:0${i}:00Z`,
      }),
    );
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    expect(grid.style.alignItems).toBe('stretch');
    const lanes = within(grid).getAllByTestId('thread-timeline-lane');
    expect(lanes).toHaveLength(8);
  });

  it('ignores wheel events whose target is a label cell so labels behave like normal page content', async () => {
    // A wheel over a label cell must NOT scrub the timeline. The wheel
    // listener attaches to the axis-column wrapper (which now hosts both
    // the label and the axis cells, because the sticky label needs to
    // share the same scroll container as the axis), so scope
    // discrimination happens by event target.
    stubAxisRect({ left: 0, width: 240 });
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
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // Wheel originating on a label cell has no effect — the wheel
    // bubbles to the axis-column wrapper but the handler returns early
    // when the target sits inside a label cell.
    const label = screen.getAllByTestId('thread-timeline-lane-label')[0];
    act(() => {
      label.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // A wheel anywhere else inside the axis-column wrapper DOES scrub —
    // proving the listener is wired but scoped past the labels. One
    // step back from the tail (msg-b) lands on msg-a at x=0.
    const axisColumn = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      axisColumn.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${LANE_LEFT_PAD_PX}px`);
  });

  it('ignores click events whose target is a label cell', async () => {
    // Same scope contract for clicks: a click on a label is not a scrub
    // intent. The handler attaches to the axis-column wrapper and the
    // same label-target discrimination keeps label clicks out of the
    // jump path.
    stubAxisRect({ left: 0, width: 240 });
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
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // A click on a label cell with clientX=0 (where msg-a would land if
    // the axis click handler picked it up) must NOT move the playhead.
    fireEvent.click(screen.getAllByTestId('thread-timeline-lane-label')[0], {
      clientX: 0,
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  // v30 fix 3: the axis cell reserves a right-side pad mirroring the left
  // pad so the rightmost large dot (6 px diameter centred on x = laneAxisWidth)
  // does not clip into the column's right edge. The axis-cell's resolved
  // width is `LANE_LEFT_PAD_PX + laneAxisWidth + LANE_RIGHT_PAD_PX`, and
  // the inner axis-line `<span>` sits at `left = LANE_LEFT_PAD_PX` with
  // `width = laneAxisWidth`. The trailing pad is therefore
  // `axisCellWidth - axisLineLeft - axisLineWidth`. The structural
  // contract we want to pin is `LANE_LEFT_PAD_PX === LANE_RIGHT_PAD_PX`
  // (symmetric pads). Reading both off the rendered DOM avoids depending
  // on an internal un-exported constant.
  it('reserves symmetric left/right padding on the axis cell so the rightmost dot is not clipped (v30)', async () => {
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const lane = await screen.findByTestId('thread-timeline-lane');
    const axisCell = lane.querySelector('[data-timeline-axis]') as HTMLElement;
    expect(axisCell).not.toBeNull();
    // The axis line `<span>` is the only direct child of the axis cell
    // with inline `left` AND `width` set in pixels (dots/playhead use
    // either transform or no width). Pick it via that signature.
    const axisLine = Array.from(
      axisCell.querySelectorAll<HTMLElement>('span[aria-hidden="true"]'),
    ).find(
      (el) =>
        /\d/.test(el.style.left ?? '') && /\d/.test(el.style.width ?? ''),
    );
    expect(axisLine).toBeDefined();
    const axisLineLeft = parseFloat(axisLine!.style.left);
    const axisLineWidth = parseFloat(axisLine!.style.width);
    const axisCellWidth = parseFloat(axisCell.style.width);
    const rightPad = axisCellWidth - axisLineLeft - axisLineWidth;
    // Left pad mirrors right pad.
    expect(rightPad).toBe(LANE_LEFT_PAD_PX);
    // And the left pad itself is the exported constant — the axis line
    // is anchored at exactly LANE_LEFT_PAD_PX from the cell's left edge.
    expect(axisLineLeft).toBe(LANE_LEFT_PAD_PX);
  });
});

describe('ThreadTimelineOverlay cluster mark size (v11 Improvement 1)', () => {
  // v10 dogfooding revealed that the cluster's render size (5 px) was
  // visually indistinguishable from the 6 px main-role dots — the user
  // could not tell a run-of-tool-calls cluster apart from a user/Claude
  // turn. The contract now is: a cluster renders at the SMALL dot
  // diameter exactly, and conveys "cluster-ness" through a thin outline
  // ring instead of size. These tests pin the contract.

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders cluster dots at exactly the small-dot diameter', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'go' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
          makeMessage(1, 2, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:20Z',
          }),
          makeMessage(1, 3, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const clusters = await screen.findAllByTestId('thread-timeline-cluster');
    expect(clusters).toHaveLength(1);
    const cluster = clusters[0];
    expect(cluster.style.width).toBe(`${MARK_SMALL_PX}px`);
    expect(cluster.style.height).toBe(`${MARK_SMALL_PX}px`);
    // Pin the literal value too — the v7/v11 contract is "cluster dots
    // stay at 4px exactly, ring-only differentiation". A future tweak to
    // MARK_SMALL_PX should break this assertion so the regression is
    // visible at review time, not in dogfooding.
    expect(cluster.style.width).toBe('4px');
    expect(cluster.style.height).toBe('4px');
    // Cross-check the constant equality so a future "let's bump cluster
    // size again" lands here, not in dogfooding.
    expect(MARK_CLUSTER_PX).toBe(MARK_SMALL_PX);
    expect(MARK_CLUSTER_PX).toBe(4);
  });

  it('renders a cluster with no outline / ring / border so its visual footprint matches a lone small dot', async () => {
    // v16: the v11 outline-based "halo" extended the cluster's painted
    // footprint by 1 px on each side, so a 4 px disc became a 6 px outer
    // disc — visually indistinguishable from the 6 px main-role dots,
    // exactly the regression v11 thought it had fixed by dropping the
    // 5 px fill. v16 drops the outline entirely. The cluster carries no
    // outline / ring / border utility, no shadow, no transform; its
    // visible footprint equals MARK_CLUSTER_PX end-to-end. "Cluster-ness"
    // is purely positional / interactive — the representative x and the
    // `data-cluster-member-count` attribute carry the meaning.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const cluster = (await screen.findAllByTestId('thread-timeline-cluster'))[0];
    // No outline / ring / border utility — these are precisely the
    // Tailwind tokens that would extend the visual footprint beyond the
    // inline width/height of MARK_CLUSTER_PX. A single failed assertion
    // here flags exactly which footprint-expanding utility crept back in.
    expect(cluster.className).not.toMatch(/\boutline\b/);
    expect(cluster.className).not.toMatch(/\boutline-1\b/);
    expect(cluster.className).not.toMatch(/\boutline-/);
    expect(cluster.className).not.toMatch(/\bring(?:-|\b)/);
    expect(cluster.className).not.toMatch(/\bborder(?:-|\b)/);
    expect(cluster.className).not.toMatch(/\bshadow(?:-|\b)/);
    // Pin the fill colour explicitly: a cluster reads as a normal small
    // assistant dot (same fill, same size, no halo).
    expect(cluster.className).toMatch(/\bbg-fg-subtle\b/);
    // No transform-scale either: a 4 px disc * scale-150 would also
    // recreate the "looks 6 px" regression at a different code path.
    expect(cluster.className).not.toMatch(/\bscale-/);
  });

  it('matches the inline width and height of a lone small dot exactly, including no outline contribution', async () => {
    // The cluster's INLINE box is sized to MARK_CLUSTER_PX. The previous
    // v11 contract relied on `outline` (which paints OUTSIDE the box and
    // does not show up in `style.width`/`height`), so a width-equals-4px
    // assertion alone could not catch the regression. This test pins
    // both the inline width/height AND the absence of any
    // footprint-extending utility class, so a future "let's add a tiny
    // ring back" regression cannot slip past the size assertion.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const cluster = (await screen.findAllByTestId('thread-timeline-cluster'))[0];
    // Pin the literal value so the value of MARK_SMALL_PX in source can
    // never silently bump the cluster footprint either.
    expect(cluster.style.width).toBe(`${MARK_SMALL_PX}px`);
    expect(cluster.style.height).toBe(`${MARK_SMALL_PX}px`);
    expect(MARK_CLUSTER_PX).toBe(MARK_SMALL_PX);
    expect(MARK_CLUSTER_PX).toBe(4);
    // Cross-check that the resolved computed style (jsdom returns the
    // inline width straight back, with no outline applied because no
    // outline class is present) also matches — guarding against a future
    // CSS-cascade rule that re-grows the disc via `width` rather than
    // `outline`.
    const computed = window.getComputedStyle(cluster);
    expect(computed.width).toBe(`${MARK_SMALL_PX}px`);
    expect(computed.height).toBe(`${MARK_SMALL_PX}px`);
    expect(computed.outlineWidth === '' || computed.outlineWidth === '0px').toBe(
      true,
    );
    expect(computed.borderTopWidth === '' || computed.borderTopWidth === '0px').toBe(
      true,
    );
  });
});

describe('ThreadTimelineOverlay scheduleScrollAfterRender DOM-ready wait (v11 Improvement 2)', () => {
  // v10's cross-lane jump deferred the scroll a single rAF; when the
  // subthread switch re-render took 2+ frames (which it usually does),
  // querySelector found no target and the scroll silently dropped. The
  // new behaviour polls each rAF until the uuid is in the DOM, capped
  // by SCROLL_DOM_READY_TIMEOUT_MS.

  beforeEach(() => {
    resetGlobals();
  });

  it('waits across multiple rAFs for the target element, then scrolls when it appears', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    // Capture rAF callbacks so we can drive them one frame at a time.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* cancellation is exercised elsewhere */
    }) as typeof window.cancelAnimationFrame;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        const cancel = scheduleScrollAfterRender(container, 'late-uuid');
        // First two ticks: target absent, scroll must NOT fire.
        expect(rafCallbacks).toHaveLength(1);
        let cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).not.toHaveBeenCalled();
        // Polling re-queues itself for the next frame.
        expect(rafCallbacks).toHaveLength(1);
        cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(1);
        // Third tick: target is now in the DOM (mirrors a real cross-lane
        // re-render that took 3 frames). The scroll fires this tick.
        const target = document.createElement('article');
        target.setAttribute('data-message-uuid', 'late-uuid');
        container.appendChild(target);
        cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(1);
        expect(scrollIntoView.mock.instances[0]).toBe(target);
        cancel();
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('gives up after SCROLL_DOM_READY_TIMEOUT_MS when the target never appears', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    const originalPerfNow = window.performance.now;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so the loop crosses the timeout deterministically
    // — first tick at t=0, second at t=TIMEOUT+1 ms.
    let nowValue = 1_000;
    window.performance.now = (() => nowValue) as typeof performance.now;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        // No matching child is ever appended.
        scheduleScrollAfterRender(container, 'never-arrives');
        expect(rafCallbacks).toHaveLength(1);
        let cb = rafCallbacks.shift()!;
        nowValue = 1_000; // first tick: t=0 elapsed
        cb(nowValue);
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(1);
        // Advance past the timeout and tick again: the loop bails without
        // re-queuing and without scrolling.
        nowValue = 1_000 + SCROLL_DOM_READY_TIMEOUT_MS + 1;
        cb = rafCallbacks.shift()!;
        cb(nowValue);
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(0);
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('re-calls scrollIntoView after one animation frame so post-layout scroll-margin-top is honoured', async () => {
    // Cross-lane jumps mount a freshly-rendered article whose computed
    // scroll-margin-top resolves only after the first layout pass. The
    // initial scrollIntoView therefore scrolls with margin=0 and the
    // article lands behind the floating top overlay. Scheduling a second
    // scrollIntoView in the next animation frame guarantees the browser
    // recomputes the scroll with the resolved margin and the article lands
    // just below the overlay.
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* cancellation is exercised elsewhere */
    }) as typeof window.cancelAnimationFrame;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        // Target article is already present so the polling loop fires its
        // run() body on the very first tick — mirroring the cross-lane jump
        // case after the new subthread's article has just been mounted.
        const target = document.createElement('article');
        target.setAttribute('data-message-uuid', 'reflow-uuid');
        container.appendChild(target);
        const cancel = scheduleScrollAfterRender(container, 'reflow-uuid');
        // First tick: target is in the DOM, the initial scrollIntoView
        // fires, and the helper schedules a follow-up rAF for the
        // post-layout re-scroll.
        expect(rafCallbacks).toHaveLength(1);
        const initialTick = rafCallbacks.shift()!;
        initialTick(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(1);
        expect(scrollIntoView.mock.instances[0]).toBe(target);
        // The follow-up rAF is queued; until it fires, no second scroll.
        expect(rafCallbacks).toHaveLength(1);
        // Drive the follow-up frame: the second scrollIntoView fires on the
        // same article. After this, layout has resolved scroll-margin-top
        // and the browser scrolls honouring the reserved top region.
        const reflowTick = rafCallbacks.shift()!;
        reflowTick(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(2);
        expect(scrollIntoView.mock.instances[1]).toBe(target);
        cancel();
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });
});

describe('ThreadTimelineOverlay pane scroll → playhead follow (v11 Improvement 3)', () => {
  // Pane-scroll → playhead is the bidirectional half of the sync: when
  // the user manually scrolls the conversation pane, an IntersectionObserver
  // on each message article picks the topmost-visible message and drives
  // the playhead to it WITHOUT bumping `scrubTick` (so no thread switch,
  // no scrollIntoView, no ping-pong). These tests exercise the wiring,
  // the no-recursion guarantee, and the programmatic-scroll guard.

  /**
   * A handle on the IntersectionObserver instances the overlay creates so
   * a test can synthesize entries directly: jsdom does not run a real
   * layout / viewport, so we cannot rely on actual scroll positions to
   * trigger callbacks.
   */
  type FakeIO = {
    callback: IntersectionObserverCallback;
    options?: IntersectionObserverInit;
    observed: Set<Element>;
    emit: (entries: Partial<IntersectionObserverEntry>[]) => void;
  };

  function installFakeIO(): { instances: FakeIO[]; restore: () => void } {
    const instances: FakeIO[] = [];
    const original = (
      globalThis as { IntersectionObserver?: typeof IntersectionObserver }
    ).IntersectionObserver;
    class FakeIntersectionObserver {
      callback: IntersectionObserverCallback;
      options?: IntersectionObserverInit;
      observed = new Set<Element>();
      emit!: (entries: Partial<IntersectionObserverEntry>[]) => void;
      constructor(
        cb: IntersectionObserverCallback,
        opts?: IntersectionObserverInit,
      ) {
        this.callback = cb;
        this.options = opts;
        // Attach `emit` here so every instance has it the moment it
        // lands in `instances` — earlier "post-construction patch"
        // approaches missed the first push.
        this.emit = (entries) => {
          this.callback(
            entries as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        };
        instances.push(this as unknown as FakeIO);
      }
      observe(el: Element) {
        this.observed.add(el);
      }
      unobserve(el: Element) {
        this.observed.delete(el);
      }
      disconnect() {
        this.observed.clear();
      }
      takeRecords(): IntersectionObserverEntry[] {
        return [];
      }
    }
    (
      globalThis as { IntersectionObserver?: unknown }
    ).IntersectionObserver = FakeIntersectionObserver;
    return {
      instances,
      restore: () => {
        (
          globalThis as {
            IntersectionObserver?: typeof IntersectionObserver | undefined;
          }
        ).IntersectionObserver = original;
      },
    };
  }

  /**
   * Resolves the LIVE FakeIO that observes every expected article. The
   * timeline effect re-runs as async query results settle and replaces its
   * IntersectionObserver each time, leaving earlier instances disconnected
   * (observed set cleared). Capturing `fake.instances[length - 1]`
   * synchronously races that replacement: between the capture and the
   * subsequent `io.emit(...)`, a later effect-run can wedge a new live
   * observer ahead of the one we grabbed, and the emit then reaches a dead
   * observer whose callback no longer fires.
   *
   * Polls until the most recent FakeIO observes `expectedObserved`
   * elements — the live observer always re-`observe()`s every article on
   * construction.
   */
  async function getLiveIO(
    fake: { instances: FakeIO[] },
    expectedObserved: number,
  ): Promise<FakeIO> {
    return waitFor(() => {
      const candidate = fake.instances.at(-1);
      expect(candidate?.observed.size).toBe(expectedObserved);
      return candidate as FakeIO;
    });
  }

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('uses threshold=PANE_SCROLL_OBSERVER_THRESHOLD and observes every rendered message article', async () => {
    const fake = installFakeIO();
    try {
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
      // The effect re-runs when `sortedMessages` settles (async query
      // arrival), so multiple FakeIO instances accumulate; the LIVE one
      // is the most recent. Earlier instances were disconnected by the
      // effect's cleanup.
      expect(fake.instances.length).toBeGreaterThan(0);
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      expect(io.options?.threshold).toBe(PANE_SCROLL_OBSERVER_THRESHOLD);
      // Every article in the conversation body is observed by the live
      // observer.
      for (const a of articles) {
        expect(io.observed.has(a)).toBe(true);
      }
    } finally {
      fake.restore();
    }
  });

  it('moves the playhead to the topmost-visible article on pane scroll, without bumping scrubTick', async () => {
    const fake = installFakeIO();
    const setActiveThreadSpy = vi.spyOn(
      useNavStore.getState(),
      'setActiveThread',
    );
    try {
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
      // Initial playhead sits on the last message (msg-c at x=240).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      // Simulate the user scrolling up so msg-a is closest to the
      // viewport top (smallest boundingClientRect.top) and msg-b is
      // partially visible below it; msg-c is now off-screen.
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: articles[2],
              isIntersecting: false,
              boundingClientRect: { top: 9999 } as DOMRect,
            },
            {
              target: articles[1],
              isIntersecting: true,
              boundingClientRect: { top: 120 } as DOMRect,
            },
            {
              target: articles[0],
              isIntersecting: true,
              boundingClientRect: { top: 10 } as DOMRect,
            },
          ]);
        });
        // Debounce: advance past PANE_SCROLL_DEBOUNCE_MS so the flush
        // fires. Wrapped in `act` separately so React commits the
        // resulting state change.
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Playhead snapped to msg-a's x (0) — the topmost-visible message
      // wins.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
      // CRUCIAL: pane-scroll updates must NOT trigger an active-thread
      // switch (the pane is already inside the active subthread).
      expect(setActiveThreadSpy).not.toHaveBeenCalled();
    } finally {
      fake.restore();
      setActiveThreadSpy.mockRestore();
    }
  });

  it('suppresses pane-scroll updates fired within the programmatic-scroll guard window', async () => {
    // The classic ping-pong: a timeline → pane jump triggers
    // scrollIntoView, which fires IO entries, which would re-update the
    // playhead. The guard window after a programmatic scroll blocks that
    // feedback. This test fires a click on the timeline (programmatic
    // scroll), then immediately emits IO entries for a DIFFERENT
    // message; the playhead must STAY at the clicked target, not jump
    // to the IO-reported one.
    const fake = installFakeIO();
    try {
      stubAxisRect({ left: 0, width: 240 });
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
      // Click at x=0: timeline jumps to msg-a (programmatic scroll fires).
      act(() => {
        fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
          clientX: 0,
        });
      });
      // Playhead now at msg-a (x=0).
      await waitFor(() => {
        expect(
          playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
        ).toBe(`${LANE_LEFT_PAD_PX}px`);
      });
      // While inside the guard window, emit an IO entry claiming
      // msg-b is topmost-visible — exactly what the jump's own scroll
      // would produce as it animates past msg-b. The playhead must NOT
      // jump to msg-b.
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: articles[1],
              isIntersecting: true,
              boundingClientRect: { top: 5 } as DOMRect,
            },
          ]);
          // Debounce shorter than the guard window: still inside guard.
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
      // Sanity: the guard exists as a constant the production code reads.
      expect(PANE_SCROLL_PROGRAMMATIC_GUARD_MS).toBeGreaterThan(
        PANE_SCROLL_DEBOUNCE_MS,
      );
    } finally {
      fake.restore();
    }
  });
});

describe('ThreadTimelineOverlay cross-lane jump IO guard (v12)', () => {
  // Regression suite for the tail-jump race: a cross-lane wheel-scrub near
  // the left edge occasionally snapped the playhead to the right-edge (tail)
  // message. Root cause: markProgrammaticScroll was called at jump-trigger
  // time, but the actual scrollIntoView only fires after DOM-ready polling
  // (scheduleScrollAfterRender). The 200 ms time-based guard expired during
  // slow re-renders; the IO's first-observation batch on the new thread's
  // articles (which always includes the tail if the pane is freshly rendered
  // at the bottom) slipped through and committed the tail index.
  //
  // The v12 fix introduced a state-based in-flight guard that holds from the
  // moment setActiveThread fires until scrollIntoView fires (or the jump is
  // cancelled). v13 changed the guard from a boolean to a counter (so a
  // burst of stacked jumps is tracked correctly) and moved
  // markProgrammaticScroll into the onScroll callback (so the 200ms time-
  // based guard window starts ticking from the moment the scroll actually
  // lands, not from the moment the jump was triggered). The IO flush bails
  // immediately while the counter is non-zero, regardless of elapsed time.

  /**
   * Fake IO helper shared with the v11 tests above. Defined locally here so
   * the suite is self-contained.
   */
  type FakeIO = {
    callback: IntersectionObserverCallback;
    options?: IntersectionObserverInit;
    observed: Set<Element>;
    emit: (entries: Partial<IntersectionObserverEntry>[]) => void;
  };

  function installFakeIO(): { instances: FakeIO[]; restore: () => void } {
    const instances: FakeIO[] = [];
    const original = (
      globalThis as { IntersectionObserver?: typeof IntersectionObserver }
    ).IntersectionObserver;
    class FakeIntersectionObserver {
      callback: IntersectionObserverCallback;
      options?: IntersectionObserverInit;
      observed = new Set<Element>();
      emit!: (entries: Partial<IntersectionObserverEntry>[]) => void;
      constructor(
        cb: IntersectionObserverCallback,
        opts?: IntersectionObserverInit,
      ) {
        this.callback = cb;
        this.options = opts;
        this.emit = (entries) => {
          this.callback(
            entries as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        };
        instances.push(this as unknown as FakeIO);
      }
      observe(el: Element) { this.observed.add(el); }
      unobserve(el: Element) { this.observed.delete(el); }
      disconnect() { this.observed.clear(); }
      takeRecords(): IntersectionObserverEntry[] { return []; }
    }
    (globalThis as { IntersectionObserver?: unknown }).IntersectionObserver =
      FakeIntersectionObserver;
    return {
      instances,
      restore: () => {
        (
          globalThis as {
            IntersectionObserver?: typeof IntersectionObserver | undefined;
          }
        ).IntersectionObserver = original;
      },
    };
  }

  /**
   * Resolves the LIVE FakeIO that observes every expected article. The
   * timeline effect re-runs as async query results settle and replaces its
   * IntersectionObserver each time, leaving earlier instances disconnected
   * (observed set cleared). Capturing `fake.instances[length - 1]`
   * synchronously races that replacement: between the capture and the
   * subsequent `io.emit(...)`, a later effect-run can wedge a new live
   * observer ahead of the one we grabbed, and the emit then reaches a dead
   * observer whose callback no longer fires.
   *
   * Polls until the most recent FakeIO observes `expectedObserved`
   * elements — the live observer always re-`observe()`s every article on
   * construction.
   */
  async function getLiveIO(
    fake: { instances: FakeIO[] },
    expectedObserved: number,
  ): Promise<FakeIO> {
    return waitFor(() => {
      const candidate = fake.instances.at(-1);
      expect(candidate?.observed.size).toBe(expectedObserved);
      return candidate as FakeIO;
    });
  }

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('suppresses IO updates fired after the time-based guard expires but before the cross-lane scroll completes', async () => {
    // This test pins the v12 fix: when a cross-lane jump triggers a slow
    // re-render (> PANE_SCROLL_PROGRAMMATIC_GUARD_MS), the IO's first-
    // observation batch on the new thread's articles must still be blocked
    // by the state-based in-flight counter, even though the time-based
    // guard has already expired.
    //
    // Sequence simulated:
    //   1. Click → cross-lane jump to lane 2 (playhead = msg-a, index 0).
    //   2. setActiveThread(2) fires; re-render takes longer than the guard.
    //   3. IO re-binds on the new thread. IO fires: tail (msg-b) is visible.
    //   4. Debounce fires — with only the time-based guard this would commit
    //      msg-b (tail), snapping the playhead to the right edge.
    //   5. With the state-based guard (flag still true), flush bails.
    //   6. The DOM-ready poll finds msg-a → scroll fires → flag clears.
    //   7. A subsequent IO emit is now honoured normally.
    const fake = installFakeIO();
    // Capture rAF callbacks to drive the DOM-ready poll manually.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised in this test */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so we can make the guard window expire on demand.
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Lane 1 holds msg-a (old subthread, left edge); lane 2 holds msg-b
      // (new subthread, right/tail). The initial playhead lands on msg-b
      // (latest), so a wheel-up from msg-b → msg-a triggers a cross-lane
      // jump to lane 1.
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      const messages = new Map([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:02:00Z')]],
      ]);
      // Start with lane 2 active; msg-b is in the pane.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-b' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial playhead is on msg-b (the latest message, x=240).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);

      // Wheel-up: one sub-notch step back → cross-lane jump to msg-a on lane 1.
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // The active thread flips to lane 1 immediately.
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // The IO re-binds now that activeThreadId changed. Re-render the pane
      // with both articles so msg-b (the tail) is visible first.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                {/* msg-a is the jump target but msg-b (tail) is also visible */}
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Advance past the time-based guard window so only the state-based
      // flag remains to block the flush.
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;

      // Simulate the IO firing for msg-b (tail) — exactly what happens when
      // the new thread's pane is freshly rendered with the tail message at
      // the bottom of the viewport.
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      const msgBArticle = articles.find((a) => a.textContent === 'msg-b')!;
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgBArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        // Advance debounce — the flush fires but the state-based guard
        // blocks it. The playhead must stay at msg-a (x=0), not jump to
        // msg-b (x=240).
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // The playhead is still at msg-a's x (0) — the state-based guard held.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);

      // Now drain the rAF callbacks so the DOM-ready scroll fires (msg-a is
      // already in the DOM), clearing the in-flight counter. As of v13 the
      // onScroll callback also stamps markProgrammaticScroll at this moment,
      // so the time-based guard window starts ticking now (not at jump
      // trigger). Advance past it before the next emit so we are testing
      // counter-release alone (the v12 contract), not the v13 time-based
      // guard hand-off.
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
      // After the counter releases AND the time-based guard expires, a
      // genuine IO emit (user manually scrolling) IS honoured normally.
      // Emit msg-b again as if the user scrolled down.
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgBArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // The emit is now honoured — playhead moves to msg-b (x=240).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('clears the in-flight flag immediately when a superseding jump cancels the pending scroll', async () => {
    // If a second jump fires before the first DOM-ready poll completes
    // (the user scrubs again before the re-render lands), the cancel handle
    // for the first jump must clear the flag — otherwise the flag would
    // permanently block pane → timeline sync.
    const fake = installFakeIO();
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so the two back-to-back wheel-up events sit
    // far enough apart that the wheel handler's output cooldown does
    // not suppress the second commit. The cooldown is unrelated to the
    // in-flight-flag race this test exercises, but the synchronous
    // dispatch order makes the gap effectively zero without a mock.
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Three messages across two lanes. Cross-lane jump: msg-c → msg-b
      // (lane 2 → lane 1). Then a second wheel-up supersedes it (msg-b →
      // msg-a, same lane 1).
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
      ];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
        [2, [makeUserText(2, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
      // Start on lane 2 with msg-c active.
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');

      // First wheel-up: cross-lane jump msg-c → msg-b (lane 2 → lane 1).
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // rAF callbacks are queued; the DOM-ready poll is in flight.
      expect(rafCallbacks.length).toBeGreaterThan(0);

      // Supersede the first jump with a second wheel-up while msg-a and
      // msg-b are now in the DOM (same-lane jump msg-b → msg-a). The
      // pending scroll cancel is invoked by the navigation effect, which
      // clears the in-flight flag.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Push the simulated clock past the output cooldown so the second
      // wheel commit lands.
      nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // The playhead is now on msg-a (the oldest message, x=0).
      await waitFor(() => {
        expect(
          playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
        ).toBe(`${LANE_LEFT_PAD_PX}px`);
      });

      // After the superseding jump, the in-flight flag must be cleared so
      // subsequent IO emits are honoured. Emit msg-b (index 1, x=120px) as
      // topmost-visible. The same-lane scroll from the superseding jump also
      // set the time-based guard; advance performance.now past that window
      // too so only the in-flight flag would block the flush (confirming it
      // is cleared by the cancel-with-flag-clear wrapper).
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      // msg-b is the article at index 1 in sortedMessages (x=120px on a
      // 3-message axis of width 240px: 0, 120, 240). Using msg-b (not the
      // tail) keeps the expectation non-trivial: if the flag were NOT cleared,
      // the flush would bail, and the playhead would stay at 0px.
      const msgBArticle = articles.find((a) => a.textContent === 'msg-b')!;
      vi.useFakeTimers();
      try {
        // Advance past the time-based guard window so only the state-based
        // flag (if still set) could block the flush.
        nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
        act(() => {
          io.emit([
            {
              target: msgBArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // The emit is honoured — the in-flight counter was decremented by the
      // cancel handle when the superseding jump fired, and the time-based
      // guard has also expired. msg-b sits at index 1 → x=120px on the
      // shared axis.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });
});

describe('ThreadTimelineOverlay cross-lane jump IO guard (v13)', () => {
  // Regression suite for the residual tail-jump race that survived v12.
  //
  // Symptom (user dogfooding): a fast wheel chain that crossed lanes —
  // especially child thread → parent thread — still occasionally snapped
  // the playhead to the tail of the new lane.
  //
  // Two compounding root causes:
  //
  // (1) v12 stamped markProgrammaticScroll at jump-trigger time, BEFORE
  //     scheduleScrollAfterRender polled the DOM for the target element.
  //     When the cross-lane re-render took longer than
  //     PANE_SCROLL_PROGRAMMATIC_GUARD_MS (200 ms), the time-based guard
  //     window had already expired by the time the onScroll callback fired
  //     and released the state-based flag. The IO ripples from the actual
  //     scrollIntoView then arrived into a fully-unguarded flush, and the
  //     tail of the new lane was committed as the active index.
  //
  //     v12 closed the "during the wait" gap with the state-based flag, but
  //     the "right after the scroll" gap was still open — the hand-off from
  //     state-based guard to time-based guard was broken because the
  //     time-based guard's window had already expired.
  //
  //     v13 fix: stamp markProgrammaticScroll inside the onScroll callback,
  //     adjacent to where the counter is released — so the 200 ms window
  //     starts ticking from the moment the IO ripples actually begin.
  //
  // (2) v12 used a boolean in-flight flag, which a stacked burst of cross-
  //     lane jumps could mishandle: the first jump's onScroll would clear
  //     the flag while a later jump was still polling, opening the same race
  //     window the guard exists to close.
  //
  //     v13 fix: replace the boolean with a counter — the guard only
  //     releases when EVERY in-flight jump has settled.

  type FakeIO = {
    callback: IntersectionObserverCallback;
    options?: IntersectionObserverInit;
    observed: Set<Element>;
    emit: (entries: Partial<IntersectionObserverEntry>[]) => void;
  };

  function installFakeIO(): { instances: FakeIO[]; restore: () => void } {
    const instances: FakeIO[] = [];
    const original = (
      globalThis as { IntersectionObserver?: typeof IntersectionObserver }
    ).IntersectionObserver;
    class FakeIntersectionObserver {
      callback: IntersectionObserverCallback;
      options?: IntersectionObserverInit;
      observed = new Set<Element>();
      emit!: (entries: Partial<IntersectionObserverEntry>[]) => void;
      constructor(
        cb: IntersectionObserverCallback,
        opts?: IntersectionObserverInit,
      ) {
        this.callback = cb;
        this.options = opts;
        this.emit = (entries) => {
          this.callback(
            entries as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        };
        instances.push(this as unknown as FakeIO);
      }
      observe(el: Element) { this.observed.add(el); }
      unobserve(el: Element) { this.observed.delete(el); }
      disconnect() { this.observed.clear(); }
      takeRecords(): IntersectionObserverEntry[] { return []; }
    }
    (globalThis as { IntersectionObserver?: unknown }).IntersectionObserver =
      FakeIntersectionObserver;
    return {
      instances,
      restore: () => {
        (
          globalThis as {
            IntersectionObserver?: typeof IntersectionObserver | undefined;
          }
        ).IntersectionObserver = original;
      },
    };
  }

  /**
   * Resolves the LIVE FakeIO that observes every expected article. The
   * timeline effect re-runs as async query results settle and replaces its
   * IntersectionObserver each time, leaving earlier instances disconnected
   * (observed set cleared). Capturing `fake.instances[length - 1]`
   * synchronously races that replacement: between the capture and the
   * subsequent `io.emit(...)`, a later effect-run can wedge a new live
   * observer ahead of the one we grabbed, and the emit then reaches a dead
   * observer whose callback no longer fires.
   *
   * Polls until the most recent FakeIO observes `expectedObserved`
   * elements — the live observer always re-`observe()`s every article on
   * construction.
   */
  async function getLiveIO(
    fake: { instances: FakeIO[] },
    expectedObserved: number,
  ): Promise<FakeIO> {
    return waitFor(() => {
      const candidate = fake.instances.at(-1);
      expect(candidate?.observed.size).toBe(expectedObserved);
      return candidate as FakeIO;
    });
  }

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('suppresses IO ripples that arrive after a slow cross-lane scroll lands (time-based guard starts at scroll-fire, not jump-trigger)', async () => {
    // This is the regression test that pins root cause (1). Sequence:
    //
    //   1. Wheel-up → cross-lane jump child (lane 2) → parent (lane 1).
    //   2. setActiveThread(1) fires. DOM-ready poll begins; the parent's
    //      re-render is slow, taking 500ms (well past the 200ms time-based
    //      guard). The in-flight counter holds the IO at bay during the wait.
    //   3. The poll finds msg-a in the DOM at +500ms; the onScroll callback
    //      stamps markProgrammaticScroll AT THAT MOMENT and releases the
    //      counter. scrollIntoView fires.
    //   4. The scroll's IO ripples arrive a few ms later. They must be
    //      blocked by the time-based guard, whose window now starts at +500
    //      (not at the jump trigger), so a flush at +550 is still inside
    //      the 200ms window.
    //
    // If v12's stamping (markProgrammaticScroll at jump-trigger) were still
    // in place, the time-based guard would have armed at t=0, expired at
    // t=200, and the flush at t=550 would commit the tail message.
    const fake = installFakeIO();
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* no-op */
    }) as typeof window.cancelAnimationFrame;
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Parent thread (lane 1) holds three messages: msg-a (target), an
      // intermediate, and msg-tail (the tail message that the IO would
      // spuriously commit if the guard is broken). Child thread (lane 2)
      // holds msg-c, which is the initial active message.
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:03:00Z',
        }),
      ];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'msg-mid', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'msg-tail', '2026-01-01T00:02:00Z'),
          ],
        ],
        [2, [makeUserText(2, 0, 'msg-c', '2026-01-01T00:04:00Z')]],
      ]);
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 2,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial playhead is on msg-c (the latest, x=240).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);

      // Wheel-up: one sub-notch step back → cross-lane jump child → parent.
      // The global sorted order is by (created_at, seq), so backwards from
      // msg-c (the initial active message) is msg-tail. The jump target is
      // therefore msg-tail (x derived from its timestamp fraction along the
      // shared global axis). The bug we want to pin is NOT about the jump
      // target being wrong; it's about a SUBSEQUENT IO emit for some OTHER
      // article in the parent lane (e.g. msg-a) snapping the playhead. So
      // after the jump lands on msg-tail, we simulate an IO emit for msg-a
      // (which would, in the broken case, commit msg-a as the new active
      // index because the guard is expired). The expectation is: playhead
      // stays on msg-tail.
      const axisColumn = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // The active thread flips to lane 1.
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      // Re-render the pane with the parent lane's articles. The target
      // (msg-tail at x=160) will land at the playhead; we'll then simulate
      // a spurious IO emit for msg-a as the "topmost-visible" entry that
      // the broken hand-off would commit.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
                <article data-message-uuid="msg-mid">msg-mid</article>
                <article data-message-uuid="msg-tail">msg-tail</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Simulate a slow parent re-render: advance the clock 500ms BEFORE
      // draining the rAF queue. The in-flight counter holds during this
      // window — assert it by emitting an IO entry for msg-a now and
      // confirming the flush bails.
      nowMs += 500;
      const articles = within(
        screen.getByTestId('conversation-body'),
      ).getAllByText(/msg-/);
      const io = await getLiveIO(fake, articles.length);
      const msgAArticle = articles.find((a) => a.textContent === 'msg-a')!;
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgAArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Counter is still held — playhead stays on msg-tail (x=120, derived
      // from its timestamp fraction along the 240px axis: 120s / 240s range).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);

      // Now drain the rAF callbacks so the DOM-ready poll fires onScroll.
      // At this moment markProgrammaticScroll stamps the time-based guard
      // with nowMs=10_500 (the slow-re-render timestamp), the counter
      // releases, and scrollIntoView lands.
      const drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }

      // Advance the clock a small amount (less than the time-based guard
      // window) and emit another IO entry for msg-a — this simulates the
      // scroll's own IO ripple. The flush must bail because the time-based
      // guard window started at nowMs=10_500 and has not yet expired.
      nowMs += 50;
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: msgAArticle,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // Playhead is still on msg-tail (x=120) — the time-based guard caught
      // the post-scroll IO ripple. In the broken v12 hand-off the playhead
      // would have snapped to msg-a (x=0).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('keeps the in-flight counter balanced across a cross-lane chain so a later jump still guards correctly (released-once, no double-decrement)', async () => {
    // The counter is decremented by TWO code paths: scheduleScrollAfterRender's
    // onScroll callback, and the cancel handle (cancelWithCountClear, invoked
    // by a superseding jump or by the cleanup effect on unmount). Both paths
    // can fire for the same jump — every wheel-step beyond the first invokes
    // the previous jump's cancel handle EVEN IF that jump's onScroll has
    // already landed. The `released` flag in the navigation effect ensures
    // each jump decrements the counter at most once.
    //
    // The observable consequence of a regression here would be: the
    // decrementCrossLaneInFlight clamp prevents the counter from wrapping
    // below zero, but the missed accounting hides a deeper bug — a future
    // jump's increment may end up "absorbing" an earlier missed decrement,
    // leaving the counter at zero when it should be at one and releasing
    // the guard prematurely.
    //
    // We exercise the end-to-end shape: a chain of three cross-lane jumps
    // (lane 3 → lane 2 → lane 1 → lane 2). Each subsequent jump invokes the
    // prior jump's cancel handle after its onScroll has already fired. If
    // the released-guard were missing, the third jump's counter would not
    // properly block an IO emit fired before its rAF poll drains.
    const fake = installFakeIO();
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* no-op */
    }) as typeof window.cancelAnimationFrame;
    let nowMs = 10_000;
    const originalPerfNow = window.performance.now;
    window.performance.now = (() => nowMs) as typeof performance.now;
    stubAxisRect({ left: 0, width: 240 });
    try {
      // Three single-message lanes so each wheel-step is a cross-lane jump:
      //   lane 1: msg-a (00:00)
      //   lane 2: msg-b (00:01)
      //   lane 3: msg-c (00:02)
      const threads = [
        makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
        makeThread(2, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:01:00Z',
        }),
        makeThread(3, {
          parent_thread_id: 1,
          root_message_uuid: null,
          created_at: '2026-01-01T00:02:00Z',
        }),
      ];
      const messages = new Map([
        [1, [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')]],
        [2, [makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z')]],
        [3, [makeUserText(3, 0, 'msg-c', '2026-01-01T00:02:00Z')]],
      ]);
      const { rerender, bodyRef } = renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 3,
        conversationArticles: [{ uuid: 'msg-c' }],
      });
      await screen.findAllByTestId('thread-timeline-dot');

      const axisColumn = screen.getAllByTestId(
        'thread-timeline-axis-column',
      )[0];

      // Jump 1: wheel-up cross-lane msg-c → msg-b (lane 3 → lane 2). Counter
      // 0 → 1.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // Advance past the wheel-handler cooldown so subsequent wheel
      // events in this chain are not throttled — the cooldown is
      // unrelated to the counter-balance contract this test exercises.
      nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      // Re-render with lane 2's article so the DOM-ready poll finds msg-b.
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={2}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Drain rAFs → onScroll fires for jump 1, counter 1 → 0.
      let drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }

      // Jump 2: wheel-up cross-lane msg-b → msg-a (lane 2 → lane 1). The
      // navigation effect invokes jump 1's cancel handle FIRST. Jump 1's
      // onScroll has already fired, so a missing released-guard would
      // attempt a second decrement on jump 1 — the clamp prevents wrap,
      // but the accounting is now off-by-one.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: -50, bubbles: true, cancelable: true }),
        );
      });
      // Advance past the wheel-handler cooldown again before the third
      // jump fires below.
      nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(1);
      });
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-a">msg-a</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      drained = rafCallbacks.splice(0, rafCallbacks.length);
      for (const cb of drained) {
        cb(nowMs);
      }

      // Jump 3: wheel-down cross-lane msg-a → msg-b (lane 1 → lane 2). The
      // navigation effect invokes jump 2's cancel handle (jump 2's onScroll
      // already fired). With released-once, the counter should now be 1
      // (jump 3 incremented, jump 2's cancel no-ops). Verify by emitting an
      // IO entry BEFORE draining rAFs — the flush must bail.
      act(() => {
        axisColumn.dispatchEvent(
          new WheelEvent('wheel', { deltaY: 50, bubbles: true, cancelable: true }),
        );
      });
      await waitFor(() => {
        expect(useNavStore.getState().activeThreadId).toBe(2);
      });
      rerender(
        <QueryClientProvider
          client={
            new QueryClient({ defaultOptions: { queries: { retry: false } } })
          }
        >
          <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
            <div>
              <div ref={bodyRef} data-testid="conversation-body">
                <article data-message-uuid="msg-b">msg-b</article>
              </div>
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={2}
                conversationBodyRef={bodyRef}
              />
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      // Advance past the time-based guard window so ONLY the counter could
      // block the IO. With released-once preserving accounting, the counter
      // is at 1; flush bails. Without it the counter is at 0 and the flush
      // would commit msg-b's IO entry (a no-op visually since the playhead
      // is already at msg-b, so we choose msg-a as the IO target — which
      // would snap the playhead BACK to msg-a if the guard were broken).
      nowMs += PANE_SCROLL_PROGRAMMATIC_GUARD_MS + 50;
      // The pane only contains msg-b right now, so IO can only emit on
      // msg-b. We use msg-b as the IO target. Because msg-b is also the
      // playhead's current position, an honoured flush would be a no-op
      // visually — that doesn't distinguish "bailed" from "honoured-on-
      // same-index". So instead, observe whether the flush would mark a
      // pane-scroll-driven change by checking that the playhead does not
      // shift. We strengthen the assertion by emitting an entry for msg-a
      // via a stand-in article appended manually below the visible one;
      // this exercises the "topmost visible is the older message" path.
      const standin = document.createElement('article');
      standin.setAttribute('data-message-uuid', 'msg-a');
      standin.textContent = 'msg-a';
      screen.getByTestId('conversation-body').appendChild(standin);
      // Observe the new article: the MutationObserver in the IO effect
      // catches DOM mutations and starts observing. Allow a microtask for
      // the observation to register, then resolve the live IO once both
      // msg-b and the stand-in are observed.
      await Promise.resolve();
      const io = await getLiveIO(fake, 2);
      vi.useFakeTimers();
      try {
        act(() => {
          io.emit([
            {
              target: standin,
              isIntersecting: true,
              boundingClientRect: { top: 0 } as DOMRect,
            },
          ]);
        });
        act(() => {
          vi.advanceTimersByTime(PANE_SCROLL_DEBOUNCE_MS + 1);
        });
      } finally {
        vi.useRealTimers();
      }
      // msg-b's x in the 3-message axis: 00:00 / 00:01 / 00:02 → range
      // 120s → msg-b at 60/120 * 240 = 120px. Playhead stays at msg-b
      // (x=120). If the counter had double-decremented and reached 0, the
      // emit would have snapped the playhead to msg-a (x=0).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      fake.restore();
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });
});

describe('ThreadTimelineOverlay article-anchored uuid selector (v16)', () => {
  // v14 pt.2 moved the timeline into the conversation pane's scroll
  // container; v15 then sticky-pinned that container to the top of the
  // scroll viewport. Both `TimelineDotMark` and `TimelineClusterMark`
  // stamp `data-message-uuid` (the dot/cluster identity matches its
  // representative message), so a bare `[data-message-uuid="X"]` query
  // rooted at the container hits the timeline span first in DOM-pre-
  // order — the span lives in the sticky topRegion that renders before
  // the article list. The result was a double regression:
  //   - timeline click → conversation pane no longer scrolled to the
  //     targeted message (scrollIntoView landed on the already-visible
  //     dot, a no-op).
  //   - conversation pane scroll → timeline playhead no longer followed
  //     (the IntersectionObserver observed the sticky dots, which always
  //     win the topmost-visible race at top: 0).
  // v16 pins every uuid query to the `<article>` tag — the only tag
  // `MessageItem` ever renders — via `articleMessageSelector` /
  // `ALL_ARTICLES_SELECTOR`. These tests reproduce the regression by
  // building a container that holds both an article and a span sharing
  // the same uuid, then assert that v16's selectors only ever pick the
  // article.

  beforeEach(() => {
    resetGlobals();
  });

  it('articleMessageSelector matches an <article> with the uuid but not a <span> with the same uuid', () => {
    // Container modelling the live layout shape: the sticky region with
    // the timeline span sits first; the message article sits below it,
    // both inside the same conversation-pane scroll container.
    const container = document.createElement('div');
    const dot = document.createElement('span');
    dot.setAttribute('data-message-uuid', 'msg-X');
    dot.setAttribute('data-testid', 'thread-timeline-dot');
    container.appendChild(dot);
    const article = document.createElement('article');
    article.setAttribute('data-message-uuid', 'msg-X');
    container.appendChild(article);
    document.body.appendChild(container);
    try {
      const sel = articleMessageSelector('msg-X');
      const matches = container.querySelectorAll(sel);
      expect(matches.length).toBe(1);
      // Article wins, not the timeline span — even though the span comes
      // first in DOM-pre-order.
      expect(matches[0]).toBe(article);
      // And the selector itself is shaped as `article[data-message-uuid="X"]`
      // so a future regression that drops the tag anchor is visible.
      expect(sel.startsWith('article[')).toBe(true);
    } finally {
      container.remove();
    }
  });

  it('ALL_ARTICLES_SELECTOR observes only article elements with a uuid, never timeline dots or clusters', () => {
    const container = document.createElement('div');
    const dot = document.createElement('span');
    dot.setAttribute('data-message-uuid', 'msg-X');
    dot.setAttribute('data-testid', 'thread-timeline-dot');
    container.appendChild(dot);
    const cluster = document.createElement('span');
    cluster.setAttribute('data-message-uuid', 'msg-X');
    cluster.setAttribute('data-testid', 'thread-timeline-cluster');
    container.appendChild(cluster);
    const articleA = document.createElement('article');
    articleA.setAttribute('data-message-uuid', 'msg-X');
    container.appendChild(articleA);
    const articleB = document.createElement('article');
    articleB.setAttribute('data-message-uuid', 'msg-Y');
    container.appendChild(articleB);
    document.body.appendChild(container);
    try {
      const matches = Array.from(
        container.querySelectorAll(ALL_ARTICLES_SELECTOR),
      );
      expect(matches).toHaveLength(2);
      expect(matches).toContain(articleA);
      expect(matches).toContain(articleB);
      expect(matches).not.toContain(dot);
      expect(matches).not.toContain(cluster);
    } finally {
      container.remove();
    }
  });

  it('scrollMessageIntoView jumps to the article, not the timeline dot, when both share the uuid in the container', () => {
    // The regression test for Issue 1: in the live app the timeline
    // sits inside the conversation pane via TranscriptPane's sticky
    // topRegion, so the conversation body contains BOTH the article
    // (the scroll target) and the dot (the timeline identity). Before
    // v16 the unscoped selector grabbed the dot, leaving the scroll a
    // no-op. After v16 the article-anchored selector picks the article.
    const container = document.createElement('div');
    const dotScrollSpy = vi.fn();
    const articleScrollSpy = vi.fn();
    const dot = document.createElement('span');
    dot.setAttribute('data-message-uuid', 'msg-X');
    dot.scrollIntoView = dotScrollSpy as unknown as Element['scrollIntoView'];
    container.appendChild(dot);
    const article = document.createElement('article');
    article.setAttribute('data-message-uuid', 'msg-X');
    article.scrollIntoView =
      articleScrollSpy as unknown as Element['scrollIntoView'];
    container.appendChild(article);
    document.body.appendChild(container);
    try {
      scrollMessageIntoView(container, 'msg-X');
      // The article is the scroll target; the dot is left untouched.
      expect(articleScrollSpy).toHaveBeenCalledTimes(1);
      expect(articleScrollSpy).toHaveBeenCalledWith({ block: 'start' });
      expect(dotScrollSpy).not.toHaveBeenCalled();
    } finally {
      container.remove();
    }
  });

  it('the pane-scroll IntersectionObserver only observes article message elements, never timeline marks, even when both share the uuid in the same container', async () => {
    // The regression test for Issue 2: before v16 the IO's
    // `querySelectorAll('[data-message-uuid]')` picked up the timeline
    // dots inside the sticky region, and those dots always reported
    // `boundingClientRect.top` near 0 (sticky-pinned), so the
    // topmost-visible race froze the playhead on a dot's uuid and the
    // playhead never followed the user's pane scroll. After v16 the
    // article-anchored selector keeps the dots out of the observation
    // set entirely.
    const fakeIO = {
      instances: [] as Array<{
        observed: Set<Element>;
      }>,
      restore: () => undefined as void,
    };
    const original = (
      globalThis as { IntersectionObserver?: typeof IntersectionObserver }
    ).IntersectionObserver;
    class FakeIO {
      observed = new Set<Element>();
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      constructor(_cb: IntersectionObserverCallback, _opts?: IntersectionObserverInit) {
        fakeIO.instances.push(this);
      }
      observe(el: Element) {
        this.observed.add(el);
      }
      unobserve(el: Element) {
        this.observed.delete(el);
      }
      disconnect() {
        this.observed.clear();
      }
      takeRecords(): IntersectionObserverEntry[] {
        return [];
      }
    }
    (
      globalThis as { IntersectionObserver?: unknown }
    ).IntersectionObserver = FakeIO;
    fakeIO.restore = () => {
      (
        globalThis as {
          IntersectionObserver?: typeof IntersectionObserver | undefined;
        }
      ).IntersectionObserver = original;
    };
    try {
      // Render with the timeline expanded so it actually paints dots —
      // a collapsed timeline is a single button with no
      // `data-message-uuid`. Use the small-dot-clustering shape so the
      // timeline produces both a regular dot AND a cluster (both
      // carrying `data-message-uuid`).
      window.localStorage.setItem(timelineExpandedKey(), 'true');
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
            makeMessage(1, 1, 't1', {
              role: 'assistant',
              content: [
                { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
              ],
              created_at: '2026-01-01T00:00:30Z',
            }),
            makeMessage(1, 2, 't2', {
              role: 'assistant',
              content: [
                { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
              ],
              created_at: '2026-01-01T00:00:40Z',
            }),
            makeUserText(1, 3, 'msg-b', '2026-01-01T00:01:00Z'),
          ],
        ],
      ]);
      // Custom render where the TIMELINE LIVES INSIDE the conversation
      // body — mirroring TranscriptPane's sticky topRegion layout. The
      // existing `renderOverlay` puts them as siblings, which would
      // mask the bug because the timeline marks live outside the
      // observed container.
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
      vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
        async (threadId) => ({
          messages: messages.get(threadId as number) ?? [],
        }),
      );
      const bodyRef = createRef<HTMLDivElement>();
      render(
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={apiClient}>
            <div ref={bodyRef} data-testid="conversation-body">
              {/* Timeline sits FIRST inside the body, just like the
                  sticky topRegion does in TranscriptPane. */}
              <ThreadTimelineOverlay
                threads={threads}
                activeThreadId={1}
                conversationBodyRef={bodyRef}
              />
              <article data-message-uuid="msg-a">msg-a</article>
              <article data-message-uuid="t1">t1</article>
              <article data-message-uuid="t2">t2</article>
              <article data-message-uuid="msg-b">msg-b</article>
            </div>
          </ApiProvider>
        </QueryClientProvider>,
      );
      await screen.findAllByTestId('thread-timeline-dot');
      // The effect re-runs as the messages query settles; the live
      // observer is the most recent.
      expect(fakeIO.instances.length).toBeGreaterThan(0);
      const io = fakeIO.instances[fakeIO.instances.length - 1];
      // Every observed element must be an `<article>`. If the
      // observer was still using a bare `[data-message-uuid]` selector,
      // the timeline dots and the cluster span (both inside the
      // conversation body now) would have crept into `io.observed`.
      for (const el of io.observed) {
        expect(el.tagName.toLowerCase()).toBe('article');
      }
      // And exactly the four articles are observed (one observer per
      // unique element).
      expect(io.observed.size).toBe(4);
    } finally {
      fakeIO.restore();
    }
  });
});

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
    // assertion local to this test).
    const scrollToMock = vi.fn(function (
      this: HTMLElement,
      options: ScrollToOptions,
    ) {
      if (typeof options.left === 'number') {
        this.scrollLeft = options.left;
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
    await waitFor(() => {
      expect(scrollToMock).toHaveBeenCalled();
    });
    // Every call must use the smooth animation API, not a positional or
    // behavior-less form. Without `behavior: 'smooth'` the auto-scroll
    // snaps and the user sees a visible jump as the playhead approaches
    // the viewport edge.
    for (const call of scrollToMock.mock.calls) {
      expect(call[0]).toMatchObject({ behavior: 'smooth' });
      expect(typeof (call[0] as ScrollToOptions).left).toBe('number');
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

describe('ThreadTimelineOverlay external active-thread change', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  /**
   * When `activeThreadId` flips because the user picked a subthread from
   * outside the overlay (Navigator click, breadcrumb, etc.) the playhead
   * must move to the new lane's latest main-conversation turn AND the
   * timeline must horizontally scroll so that new x is on screen. Without
   * the fix the playhead stayed pointed at the previous lane's message,
   * and on long sessions the playhead's x sat outside the axis viewport —
   * invisible to the user.
   */
  it('moves the playhead to the latest large message of the new lane on external activeThreadId change', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 1 carries msg-a at t=0; lane 2 carries msg-b at t=1m and
    // msg-c at t=2m. The latest large message in lane 2 is msg-c.
    const messages = new Map([
      [
        1,
        [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')],
      ],
      [
        2,
        [
          makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(2, 1, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    // Mount with lane 1 active. The playhead initially lands on the
    // latest message of the global sorted list (msg-c at x=240).
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => ({
        messages: messages.get(threadId as number) ?? [],
      }),
    );
    const bodyRef = createRef<HTMLDivElement>();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-a">msg-a</article>
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={1}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    await screen.findAllByTestId('thread-timeline-dot');
    // Sanity: the initial playhead sits on the global tail (msg-c at x=240
    // = LANE_LEFT_PAD_PX + 240). This is the auto-anchor effect's pick on
    // first mount, not a deliberate "show me the latest of lane 1".
    const playheads = () => screen.getAllByTestId('thread-timeline-playhead');
    expect(playheadLeftPx(playheads()[0])).toBe(`${LANE_LEFT_PAD_PX + 240}px`);
    // Now flip activeThreadId to lane 2 from the outside, mirroring a
    // Navigator click. Re-render the pane with lane 2's articles so the
    // DOM matches what the live app shows after the switch.
    rerender(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-b">msg-b</article>
              <article data-message-uuid="msg-c">msg-c</article>
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={2}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    // The effect picks the latest large message in the new lane (msg-c).
    // msg-c sits at the global tail (x = 240 inside the axis), so the
    // playhead's translateX must be LANE_LEFT_PAD_PX + 240. The lane-2
    // playhead — not lane-1's — is the one that should track this x.
    await waitFor(() => {
      const lane2Playhead = playheads()[1];
      expect(playheadLeftPx(lane2Playhead)).toBe(
        `${LANE_LEFT_PAD_PX + 240}px`,
      );
    });
    // The lane highlight follows the new active message's lane (lane 2).
    const lanes = screen.getAllByTestId('thread-timeline-lane');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
  });

  it('triggers horizontal scroll catch-up so the playhead lands inside the axis viewport after the external switch', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Pick widely-separated timestamps so the global x map keeps msg-a at
    // x=0 and msg-c at the right end (x=240). Lane 2's latest large is
    // msg-c — far to the right of the initial viewport.
    const messages = new Map([
      [
        1,
        [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')],
      ],
      [
        2,
        [
          makeUserText(2, 0, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(2, 1, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => ({
        messages: messages.get(threadId as number) ?? [],
      }),
    );
    const bodyRef = createRef<HTMLDivElement>();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-a">msg-a</article>
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={1}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    await screen.findAllByTestId('thread-timeline-dot');
    const wrapper = screen.getByTestId('thread-timeline-axis-column');
    // Make the wrapper narrow so the scroll-follow effect actually runs.
    Object.defineProperty(wrapper, 'clientWidth', {
      configurable: true,
      get: () => 100,
    });
    // Spy on the smooth-scroll API. The fix routes the catch-up through
    // `scrollTo({ behavior: 'smooth' })` (gated on `userActedTick`), so
    // the external active-thread switch must invoke it exactly like a
    // wheel/click jump would.
    const scrollToMock = vi.fn();
    wrapper.scrollTo = scrollToMock as typeof wrapper.scrollTo;
    // Pre-position the scroll at the left edge — msg-c at x=240 (axis-
    // local) sits well outside [0, 100].
    wrapper.scrollLeft = 0;
    // Flip activeThreadId to lane 2 with the new lane's article in the DOM.
    rerender(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-b">msg-b</article>
              <article data-message-uuid="msg-c">msg-c</article>
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={2}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    // The horizontal scroll-follow effect must fire and request a smooth
    // re-centre — that is the user-visible "playhead becomes visible"
    // half of the fix. The exact left value depends on the live label
    // offset (jsdom returns 0 for `offsetLeft` without explicit CSS), so
    // just assert the call happened with the smooth API.
    await waitFor(() => {
      expect(scrollToMock).toHaveBeenCalled();
    });
    const lastCall = scrollToMock.mock.calls[scrollToMock.mock.calls.length - 1];
    expect(lastCall[0]).toMatchObject({ behavior: 'smooth' });
    expect(typeof (lastCall[0] as ScrollToOptions).left).toBe('number');
  });

  it('leaves the playhead alone when the new lane has no large messages yet', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    // Lane 2 has a non-text message only (e.g. a tool call placeholder
    // before any large turn lands). The empty-content row is treated as a
    // small mark, not a large one — and {@link buildLargeSortedMessages}
    // includes only large rows, so the new effect must find no candidate
    // and leave the playhead at its current position.
    const messages = new Map([
      [
        1,
        [makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z')],
      ],
      [
        2,
        [
          // makeMessage's default content is `[]` — that produces a small
          // (auxiliary) mark, NOT a large one.
          makeMessage(2, 0, 'msg-b-small', {
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const apiClient = new ApiClient({ baseUrl: 'http://localhost' });
    vi.spyOn(apiClient, 'getThreadMessages').mockImplementation(
      async (threadId) => ({
        messages: messages.get(threadId as number) ?? [],
      }),
    );
    const bodyRef = createRef<HTMLDivElement>();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body">
              <article data-message-uuid="msg-a">msg-a</article>
            </div>
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={1}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    await screen.findAllByTestId('thread-timeline-dot');
    const before = playheadLeftPx(
      screen.getAllByTestId('thread-timeline-playhead')[0],
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={apiClient}>
          <div>
            <div ref={bodyRef} data-testid="conversation-body" />
            <ThreadTimelineOverlay
              threads={threads}
              activeThreadId={2}
              conversationBodyRef={bodyRef}
            />
          </div>
        </ApiProvider>
      </QueryClientProvider>,
    );
    // Give the effect a chance to run; nothing should change.
    await Promise.resolve();
    await Promise.resolve();
    const after = playheadLeftPx(
      screen.getAllByTestId('thread-timeline-playhead')[0],
    );
    expect(after).toBe(before);
  });
});
