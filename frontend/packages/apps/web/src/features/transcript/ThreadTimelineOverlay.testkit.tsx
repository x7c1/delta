/**
 * Shared fixtures and helpers for the `ThreadTimelineOverlay.*.test.tsx`
 * family. The overlay's unit suites live in several behavior-grouped files
 * (chrome, lanes, playhead, step navigation, scroll follow, pane follow,
 * cross-lane jump, external thread change); every file renders the same
 * overlay-plus-conversation-body harness and shares the thread/message
 * factories and jsdom layout stubs defined here.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import { createRef, type RefObject } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  render,
  screen,
  waitFor,
  type RenderResult,
} from '@testing-library/react';
import { expect, vi } from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { Message, Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { ThreadTimelineOverlay } from './ThreadTimelineOverlay';
import {
  resetTimelineExpandedForTests,
  TIMELINE_EXPANDED_SUBKEY,
} from './useTimelineExpanded';
import { sessionScopedKey } from '../../store/sessionScopedStorage';

/**
 * Session id the test fixtures pin every thread / message to (see
 * `makeThread` / `makeMessage`). The overlay reads the focused session id
 * from `navStore` to scope its expand preference, so every test sets this
 * value as the focus in `resetGlobals` — otherwise the hook falls back to
 * the in-memory-only `null` branch and never persists.
 */
export const TEST_SESSION_ID = 'session-1';

/**
 * Compose the localStorage key the overlay actually writes to for the
 * current test session. Wraps the helper's `(sessionId, subKey)` shape so
 * each test reads `localStorage.getItem(timelineExpandedKey())` rather than
 * spelling the layout out by hand.
 */
export function timelineExpandedKey(sessionId: string = TEST_SESSION_ID): string {
  return sessionScopedKey(sessionId, TIMELINE_EXPANDED_SUBKEY);
}

export function makeThread(
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

export function makeMessage(
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
    provider_item_id: null,
    ...overrides,
  };
}

/**
 * A user-role message carrying a single text block — i.e. a "large"
 * main-conversation turn the wheel step navigation targets. Tests that
 * exercise wheel stepping use this so the messages land in the
 * `largeSortedMessages` subset (the wheel skips auxiliary tool/meta marks).
 */
export function makeUserText(
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
export function renderOverlay({
  threads,
  messagesByThread,
  activeThreadId = null,
  conversationArticles = [] as { uuid: string }[],
}: {
  threads: Thread[];
  messagesByThread: Map<number, Message[]>;
  activeThreadId?: number | null;
  conversationArticles?: { uuid: string }[];
  // Explicit return type: the inferred RenderResult drags non-portable
  // pretty-format type paths into the exported signature (TS2742).
}): RenderResult & {
  apiClient: ApiClient;
  bodyRef: RefObject<HTMLDivElement>;
} {
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
export function resetGlobals() {
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
export function stubAxisRect(rect: Partial<DOMRect>): void {
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
export function playheadLeftPx(el: HTMLElement): string {
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

/**
 * Wait until the first lane's playhead has settled at `expectedPx`.
 *
 * Rendering a mark is not the same event as landing the playhead on it: the
 * marks appear in the commit that brings the messages in, while the mount
 * auto-anchor that positions the playhead runs in that commit's effects. React
 * defers effects to a later task whenever a commit overruns the scheduler's
 * frame budget, so on a loaded machine `await findAllByTestId('…-dot')` can
 * return while the playhead is still at its pre-anchor x=0. A single-shot read
 * there samples that intermediate state and the whole test then measures a
 * step from the wrong origin.
 *
 * Tests therefore establish the anchored starting position through this
 * condition-based wait instead. The asserted coordinate is unchanged — only
 * the "first value seen" is replaced by "the value it settles on".
 */
export async function waitForPlayheadAt(expectedPx: string): Promise<void> {
  await waitFor(() => {
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(expectedPx);
  });
}
