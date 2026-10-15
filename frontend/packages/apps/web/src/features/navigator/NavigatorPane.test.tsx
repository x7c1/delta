/**
 * The session rows: the running indicator, the thread trees the list carries,
 * where a launched session is placed, and the settings entry.
 *
 * Shared fixtures live in NavigatorPane.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { act, fireEvent, screen, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { SESSION_ID } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SessionListItem } from '@delta/wire-gen';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { makeItem, renderPane, server } from './NavigatorPane.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('NavigatorPane per-session running indicator', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      unread: {},
    });
    useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it('shows the running indicator only on the session with an in-flight turn', () => {
    // Only the first session has an active turn.
    useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 1: true } } });

    renderPane();

    const rows = screen.getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    // The active session's row shows the indicator; the idle one does not.
    expect(
      within(rows[0]).queryByTestId('session-running'),
    ).toBeInTheDocument();
    expect(
      within(rows[1]).queryByTestId('session-running'),
    ).not.toBeInTheDocument();
    // Exactly one row carries the indicator overall.
    expect(screen.getAllByTestId('session-running')).toHaveLength(1);
  });

  it('shows no running indicator when no session has an in-flight turn', () => {
    renderPane();

    expect(screen.queryByTestId('session-running')).not.toBeInTheDocument();
  });

  it('renders no global footer running indicator', () => {
    // The footer spinner used to appear whenever any turn was in flight; it has
    // been replaced by the per-row indicator above.
    useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 1: true } } });

    renderPane();

    // The only "running" text now lives inside a per-session row (the
    // visually-hidden label), never as a standalone footer spinner.
    const runningRows = screen.getAllByTestId('session-running');
    expect(runningRows).toHaveLength(1);
  });
});

describe('NavigatorPane thread trees from the session list', () => {
  /** A list item for `id` whose threads are its trunk plus `branches`. */
  function itemWithBranches(
    id: string,
    mainThreadId: number,
    branches: string[],
  ): SessionListItem {
    const item = makeItem(id, mainThreadId);
    return {
      ...item,
      threads: [
        ...item.threads,
        ...branches.map((title, index) => ({
          id: mainThreadId + index + 1,
          session_id: id,
          title,
          parent_thread_id: mainThreadId,
          root_message_uuid: `${id}-root-${index}`,
          created_at: '2026-01-01T00:00:00Z',
          last_activity_at: null,
        })),
      ],
    };
  }

  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      unread: {},
      spawns: [],
    });
    useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it("draws every row's sub-thread tree from the list without requesting any session's threads", async () => {
    const listed = [
      itemWithBranches('tree-a', 10, ['alpha one', 'alpha two']),
      itemWithBranches('tree-b', 20, ['beta one']),
      itemWithBranches('tree-c', 30, ['gamma one']),
    ];
    const threadRequests: string[] = [];
    server.use(
      http.get('*/api/sessions', () =>
        HttpResponse.json({ sessions: listed, next_cursor: null }),
      ),
      http.get('*/api/sessions/:id/threads', ({ params }) => {
        threadRequests.push(String(params.id));
        return HttpResponse.json({ threads: [] });
      }),
    );
    const page = await new ApiClient({
      baseUrl: 'http://localhost',
    }).getSessions({});

    const { unmount } = renderPane(page.sessions);

    // Each row shows its own sub-threads, and only its own.
    const rowOf = (repo: string) =>
      screen
        .getAllByRole('listitem')
        .find((row) => row.textContent?.includes(repo)) as HTMLElement;
    for (const [repo, mine, others] of [
      ['dev/tree-a', ['alpha one', 'alpha two'], ['beta one', 'gamma one']],
      ['dev/tree-b', ['beta one'], ['alpha one', 'gamma one']],
      ['dev/tree-c', ['gamma one'], ['alpha one', 'beta one']],
    ] as const) {
      for (const title of mine) {
        expect(within(rowOf(repo)).getByText(title)).toBeInTheDocument();
      }
      for (const title of others) {
        expect(within(rowOf(repo)).queryByText(title)).not.toBeInTheDocument();
      }
    }

    // Rows mounting again (as an unfocused row does when it scrolls back into
    // view) read the list too.
    unmount();
    renderPane(page.sessions);
    expect(screen.getByText('beta one')).toBeInTheDocument();

    // Give any request a row might have started time to reach the server.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(threadRequests).toEqual([]);
  });
});

describe('NavigatorPane launched-session placement', () => {
  // More open sessions than the windowed list renders in jsdom's 600px
  // viewport (plus overscan), so a card listed after all of them is unmounted.
  const OPEN_COUNT = 40;
  const openIds = Array.from(
    { length: OPEN_COUNT },
    (_, index) => `open-${String(index).padStart(2, '0')}`,
  );
  const FAILED_ID = 'failed-launch';
  const failedItem: SessionListItem = {
    ...makeItem(FAILED_ID, 999),
    open: false,
  };
  failedItem.session = { ...failedItem.session, status: 'failed' };
  // The server's open-first order: every open session, then the failed one.
  const serverOrder = [
    ...openIds.map((id, index) => makeItem(id, index + 100)),
    failedItem,
  ];

  function failedSpawn(sessionId: string) {
    return {
      sessionId,
      threadId: 999,
      text: 'first message',
      firstSendId: 1,
      workdir: '/projects/app',
      launchOptionIds: [],
      provider: 'claude' as const,
      worktree: null,
      pullRequestNumber: null,
      status: 'failed' as const,
    };
  }

  /** The session ids of the mounted cards, top to bottom, matched by repo name. */
  function renderedIds(): string[] {
    return screen
      .getAllByRole('listitem')
      .map(
        (row) =>
          [...openIds, FAILED_ID].find((id) =>
            row.textContent?.includes(`dev/${id}`),
          ) ?? '?',
      );
  }

  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      unread: {},
      spawns: [],
    });
    useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it('leaves a failed card it did not launch below the open sessions, out of the window', () => {
    renderPane(serverOrder);

    const ids = renderedIds();
    expect(ids.length).toBeLessThan(serverOrder.length);
    expect(ids).toEqual(openIds.slice(0, ids.length));
    expect(
      screen.queryByRole('status', { name: 'Failed' }),
    ).not.toBeInTheDocument();
  });

  it('pins the failed card of its own launch to the top, keeping the open-first order of the rest', () => {
    useLiveStore.setState({ spawns: [failedSpawn(FAILED_ID)] });

    renderPane(serverOrder);

    const rows = screen.getAllByRole('listitem');
    expect(
      within(rows[0]).getByRole('status', { name: 'Failed' }),
    ).toBeInTheDocument();
    const ids = renderedIds();
    expect(ids[0]).toBe(FAILED_ID);
    expect(ids.slice(1)).toEqual(openIds.slice(0, ids.length - 1));
  });

  it('returns the card to the server order once the launch is no longer tracked', () => {
    useLiveStore.setState({ spawns: [failedSpawn(FAILED_ID)] });
    renderPane(serverOrder);
    expect(renderedIds()[0]).toBe(FAILED_ID);

    // Retry or Remove clears the tracked spawn.
    act(() => useLiveStore.getState().clearSpawn(FAILED_ID));

    expect(renderedIds()).not.toContain(FAILED_ID);
    expect(renderedIds()[0]).toBe(openIds[0]);
  });
});

describe('NavigatorPane settings entry', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
    });
    useNavStore.setState({ settingsOpen: false });
  });

  it('opens the settings overlay when the footer entry is clicked', () => {
    renderPane();

    const entry = screen.getByTestId('settings-entry');
    expect(entry).toHaveAttribute('aria-pressed', 'false');

    fireEvent.click(entry);

    expect(useNavStore.getState().settingsOpen).toBe(true);
  });

  it('marks the entry pressed while the settings overlay is open', () => {
    useNavStore.setState({ settingsOpen: true });

    renderPane();

    expect(screen.getByTestId('settings-entry')).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });
});
