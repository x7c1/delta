import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import type { ComponentProps } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';
import { createHandlers, SESSION_ID } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SessionListItem } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { SessionNode } from './SessionNode';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => {
  server.resetHandlers();
  // Running and unread are read from the live store (thread-keyed), OR-aggregated
  // over the session's threads onto the collapsed row; the permission badge is
  // read from `notices`. Reset all of them between cases.
  useLiveStore.setState({
    runningThreads: {},
    unread: {},
    runningSubagents: {},
    notices: {},
  });
  // The card header and thread tree now drive focus through `useNavStore`
  // directly (rather than an `onFocus` prop), so clear the nav selection
  // between cases too.
  useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
});
afterAll(() => server.close());

/** Flag the session's main thread (id 1) running in the store. */
function setRunning() {
  useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 1: true } } });
}

/**
 * Flag a sub-thread (id 2, i.e. NOT the main thread) running in the store.
 * The header spinner is keyed off the main thread alone, so this lets a test
 * assert that sub-thread activity does not light the header.
 */
function setSubThreadRunning() {
  useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 2: true } } });
}

/** Flag the session's main thread (id 1) unread in the store. */
function setUnread() {
  useLiveStore.setState({ unread: { 1: 1 } });
}

/**
 * Record a pending permission notice for the session in the live store. The
 * row now reads its permission state from `notices` with a narrow selector
 * (rather than taking a `needsPermission` prop), so tests seed the store.
 */
function setNeedsPermission() {
  useLiveStore.setState({
    notices: {
      [SESSION_ID]: [
        {
          kind: 'permission',
          requestId: 1,
          toolName: 'Bash',
          toolInput: '{}',
          dismissed: false,
          queued: [],
          pendingCount: 1,
        },
      ],
    },
  });
}

/** Record a background subagent running on the session's main thread (id 1). */
function setRunningSubagent() {
  useLiveStore.setState({
    runningSubagents: {
      [SESSION_ID]: [
        {
          threadId: 1,
          toolUseId: 'toolu_bg',
          subagentType: null,
          description: null,
          background: true,
        },
      ],
    },
  });
}

const item: SessionListItem = {
  session: {
    id: SESSION_ID,
    cwd: '/home/dev/project',
    transcript_path: '',
    title: null,
    status: 'active',
    created_at: '2026-01-01T00:00:00Z',
    branch_at_launch: 'main',
    repo_root: '/home/dev/project',
    repository_display_name: 'dev/project',
    provider: 'claude',
    provider_session_id: null,
    provider_thread_id: null,
  },
  open: true,
  main_thread_id: 1,
  last_activity_at: '2026-01-01T00:00:00Z',
};

/**
 * The same row before its launch registered: listed from the moment its first
 * send was accepted, so `status: 'spawning'` and no live pane yet.
 */
const spawningItem: SessionListItem = {
  ...item,
  session: { ...item.session, status: 'spawning' },
  open: false,
};

function renderNode(props: Partial<ComponentProps<typeof SessionNode>>) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <SessionNode item={item} isFocused={false} {...props} />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('SessionNode running indicator', () => {
  it('renders the running indicator when the main thread of the session is running', () => {
    // `setRunning` flags the main thread (id 1) running; the header spinner is
    // scoped to the main thread, so this is the trigger case for the spinner.
    setRunning();
    renderNode({});

    const running = screen.getByTestId('session-running');
    expect(running).toBeInTheDocument();
    // The compact glyph is aria-hidden, so an accessible "running" label is
    // paired with it for assistive tech.
    expect(running).toHaveTextContent('running');
  });

  it('does not render the running indicator when no thread is running', () => {
    renderNode({});

    expect(screen.queryByTestId('session-running')).not.toBeInTheDocument();
  });

  it('does not light the header when only a sub-thread is running', () => {
    // Sub-thread activity has its own spinner inside the ThreadTree below; the
    // header spinner is keyed off the main thread alone to avoid double-marking
    // the same activity.
    setSubThreadRunning();
    renderNode({});

    expect(screen.queryByTestId('session-running')).not.toBeInTheDocument();
  });

  it('shows the running indicator and the permission badge together', () => {
    setRunning();
    setNeedsPermission();
    renderNode({});

    expect(screen.getByTestId('session-running')).toBeInTheDocument();
    expect(
      screen.getByTestId('session-permission-badge'),
    ).toBeInTheDocument();
  });

  it('shows the running indicator when only a subagent is running on the main thread', () => {
    // The launching turn has ended but its background subagent keeps working;
    // the main thread still reads as running (`setRunningSubagent` records its
    // subagent against thread id 1), so the row shows the spinner.
    setRunningSubagent();
    renderNode({});

    expect(screen.getByTestId('session-running')).toBeInTheDocument();
  });
});

describe('SessionNode unread indicator', () => {
  it('renders the unread dot when a thread is unread and not running', () => {
    setUnread();
    renderNode({});

    const unread = screen.getByTestId('session-unread');
    expect(unread).toBeInTheDocument();
    // The dot itself is aria-hidden, so an accessible "unread" label is paired
    // with it for assistive tech.
    expect(unread).toHaveTextContent('unread');
  });

  it('does not render the unread dot when no thread is unread', () => {
    renderNode({});

    expect(screen.queryByTestId('session-unread')).not.toBeInTheDocument();
  });

  it('hides the unread dot while running (running takes precedence)', () => {
    setUnread();
    setRunning();
    renderNode({});

    // A session processing again shows the live spinner, not a stale dot.
    expect(screen.getByTestId('session-running')).toBeInTheDocument();
    expect(screen.queryByTestId('session-unread')).not.toBeInTheDocument();
  });

  it('suppresses the unread dot while a launched subagent is still running', () => {
    // The turn completed (unread bumped) but its background subagent is still
    // working: the thread reads as running, so the row shows the spinner and not
    // the "done while you were away" dot until the subagent finishes.
    setUnread();
    setRunningSubagent();
    renderNode({});

    expect(screen.getByTestId('session-running')).toBeInTheDocument();
    expect(screen.queryByTestId('session-unread')).not.toBeInTheDocument();
  });

  it('lights the running spinner from a subagent alone, with no turn running', () => {
    // The navigator carries no separate subagent badge: a running subagent
    // (background here, so it outlives its launching turn) folds into the row's
    // "running", which is the single signal the user needs from another session
    // — "this one is still working" — regardless of what is running inside.
    setRunningSubagent();
    renderNode({});

    expect(screen.getByTestId('session-running')).toBeInTheDocument();
    expect(
      screen.queryByTestId('session-subagent-badge'),
    ).not.toBeInTheDocument();
  });
});

describe('SessionNode repo line', () => {
  it('renders the backend repository_display_name and shows repo_root in the tooltip', () => {
    renderNode({});

    const repo = screen.getByTestId('session-repo');
    expect(repo).toHaveTextContent('dev/project');
    // The short label is the primary path; the tooltip carries the full
    // working-tree path so the user can still see exactly where the
    // session is running.
    expect(repo).toHaveAttribute('title', '/home/dev/project');
    // Both the primary and the fallback path RTL-truncate: a long `org/repo`
    // should clip the org and keep the repo name (`…/repo`), not the other
    // way around.
    expect(repo.className).toContain('[direction:rtl]');
  });

  it('falls back to the cwd basename and RTL-truncates when repository_display_name is null', () => {
    // A legacy row (predates the column) OR a session launched outside any
    // git repo: backend sends `repository_display_name: null`, frontend
    // renders the cwd basename instead. RTL truncation is shared with the
    // primary path so the fallback also keeps the meaningful tail of a long
    // local path.
    const legacy: SessionListItem = {
      ...item,
      session: {
        ...item.session,
        repository_display_name: null,
        repo_root: null,
        cwd: '/Users/x7c1/projects/local-only',
      },
    };
    renderNode({ item: legacy });

    const repo = screen.getByTestId('session-repo');
    expect(repo).toHaveTextContent('local-only');
    expect(repo).toHaveAttribute('title', '/Users/x7c1/projects/local-only');
    expect(repo.className).toContain('[direction:rtl]');
  });

  it('omits the repo span entirely when no usable label can be derived', () => {
    // Neither a backend label nor a cwd basename — the line-2 left span is
    // omitted (the rest of the row still renders).
    const empty: SessionListItem = {
      ...item,
      session: {
        ...item.session,
        repository_display_name: null,
        repo_root: null,
        cwd: '',
      },
    };
    renderNode({ item: empty });

    expect(screen.queryByTestId('session-repo')).not.toBeInTheDocument();
  });

  it('renders the last-activity time with condensed font-stretch', () => {
    // The timestamp is shown in tabular-nums plus a `font-stretch: condensed`
    // hint so timestamps stay compact next to the repo label on a narrow row.
    // The class is honoured only by variable fonts that ship a condensed axis;
    // it is a no-op fallback otherwise, but the explicit hint stays.
    renderNode({});

    const lastActivity = screen.getByTestId('session-last-activity');
    expect(lastActivity.className).toContain('[font-stretch:condensed]');
  });
});

describe('SessionNode branch display', () => {
  it('shortens a delta-managed branch and exposes the full name on hover', () => {
    // Sessions delta spawns get a `delta-<uuid>` branch on disk; the inline
    // span shows a readable 8-char form while the `title` keeps the full
    // identifier recoverable.
    const branch = 'delta-019ef8ff-76aa-7870-a0dd-3a5856d28d79';
    renderNode({
      item: {
        ...item,
        session: { ...item.session, branch_at_launch: branch },
      },
    });

    const span = screen.getByTestId('session-branch');
    expect(span).toHaveTextContent('019ef8ff');
    expect(span.textContent).not.toContain('delta-');
    expect(span.getAttribute('title')).toBe(branch);
  });

  it('leaves a user-named branch unchanged', () => {
    // The helper is a no-op for any name that does not match the
    // `delta-<uuid>` pattern, so plain branches render verbatim.
    renderNode({
      item: {
        ...item,
        session: { ...item.session, branch_at_launch: 'feat/example' },
      },
    });

    const span = screen.getByTestId('session-branch');
    expect(span).toHaveTextContent('feat/example');
    expect(span.getAttribute('title')).toBe('feat/example');
  });
});

describe('SessionNode status', () => {
  it('reads Open for a live session and Closed for a torn-down one', () => {
    const { unmount } = renderNode({});
    expect(screen.getByRole('status', { name: 'Open' })).toBeInTheDocument();
    unmount();

    renderNode({ item: { ...item, open: false } });
    expect(screen.getByRole('status', { name: 'Closed' })).toBeInTheDocument();
  });

  it('reads Starting while the session’s launch has not registered', () => {
    // A starting session is listed from the moment its first send is accepted.
    // It has no live pane, so it would otherwise read `Closed` — which invites
    // a resume the server refuses. `Starting` is the third state: something to
    // wait out, not to act on.
    renderNode({ item: spawningItem });

    expect(screen.getByRole('status', { name: 'Starting' })).toBeInTheDocument();
    expect(
      screen.queryByRole('status', { name: 'Closed' }),
    ).not.toBeInTheDocument();
  });
});

describe('SessionNode card styling', () => {
  it('applies no color transition to the card, so the hover highlight is instant', () => {
    // Hover feedback is a "respond now" affordance: a `transition-colors` on
    // the card would animate the hover-in/out border change over Tailwind's
    // default 150 ms, making the highlight visibly lag the cursor (most
    // noticeable on WebKitGTK). The card must carry no color transition so the
    // hover and focused-state border/background changes apply immediately.
    renderNode({});

    const card = screen.getByTestId('session-card');
    expect(card.className).not.toContain('transition-colors');
  });
});

describe('SessionNode provider marker', () => {
  it('tints the kebab trigger in the Claude hue and names the provider', () => {
    // The shared `item` fixture runs on Claude.
    renderNode({});

    // The kebab trigger doubles as the card's provider marker: its resting
    // text color is the provider hue instead of the default subtle gray.
    const trigger = screen.getByRole('button', {
      name: /^Session actions for .* \(Claude Code session\)$/,
    });
    expect(trigger.className).toContain('text-provider-claude');
    expect(trigger.className).not.toContain('text-provider-codex');
  });

  it('tints the kebab trigger in the Codex hue for a Codex session', () => {
    renderNode({
      item: {
        ...item,
        session: { ...item.session, provider: 'codex' },
      },
    });

    const trigger = screen.getByRole('button', {
      name: /^Session actions for .* \(Codex session\)$/,
    });
    expect(trigger.className).toContain('text-provider-codex');
    expect(trigger.className).not.toContain('text-provider-claude');
  });
});

describe('SessionNode kebab menu', () => {
  // jsdom does not implement `navigator.clipboard`, so install a stub holding a
  // `vi.fn()` writeText. `configurable: true` lets afterAll restore the original
  // descriptor cleanly.
  const writeText = vi.fn<(text: string) => Promise<void>>();
  const originalClipboard = Object.getOwnPropertyDescriptor(
    navigator,
    'clipboard',
  );
  beforeAll(() => {
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
      writable: true,
    });
  });
  afterAll(() => {
    if (originalClipboard) {
      Object.defineProperty(navigator, 'clipboard', originalClipboard);
    } else {
      // The property did not exist before — drop the stub.
      delete (navigator as unknown as { clipboard?: unknown }).clipboard;
    }
  });
  beforeEach(() => {
    writeText.mockReset();
    writeText.mockResolvedValue(undefined);
  });

  it('exposes both "Copy session ID" and "Close" while the session is open', () => {
    renderNode({});

    fireEvent.click(
      screen.getByRole('button', { name: /Session actions for/ }),
    );

    expect(
      screen.getByRole('menuitem', { name: 'Copy session ID' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: 'Close' }),
    ).toBeInTheDocument();
  });

  it('exposes only "Copy session ID" when the session is closed', () => {
    // The menu trigger must still be enabled for a closed session — copying the
    // id is useful regardless of whether the session is running.
    renderNode({ item: { ...item, open: false } });

    fireEvent.click(
      screen.getByRole('button', { name: /Session actions for/ }),
    );

    expect(
      screen.getByRole('menuitem', { name: 'Copy session ID' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Close' }),
    ).not.toBeInTheDocument();
  });

  it('offers no "Close" while the session is still starting', () => {
    // A starting session is listed (and clickable) from the moment its first
    // send is accepted, but nothing is bound to it yet: there is no pane to
    // tear down, and closing it would leave the launch coming up orphaned.
    renderNode({ item: spawningItem });

    fireEvent.click(
      screen.getByRole('button', { name: /Session actions for/ }),
    );

    expect(
      screen.getByRole('menuitem', { name: 'Copy session ID' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Close' }),
    ).not.toBeInTheDocument();
  });

  it('writes the session id to the clipboard when "Copy session ID" is picked', () => {
    renderNode({});

    fireEvent.click(
      screen.getByRole('button', { name: /Session actions for/ }),
    );
    fireEvent.click(
      screen.getByRole('menuitem', { name: 'Copy session ID' }),
    );

    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith(item.session.id);
  });

  it('issues a close request for the session when "Close" is picked', async () => {
    // The close mutation is now wired inside the row (previously an `onClose`
    // prop), so assert the click reaches `POST /api/sessions/{id}/close` with
    // this session's id.
    const closed = vi.fn<(id: string | readonly string[]) => void>();
    server.use(
      http.post('*/api/sessions/:id/close', ({ params }) => {
        closed(params.id);
        return new HttpResponse(null, { status: 204 });
      }),
    );
    renderNode({});

    fireEvent.click(
      screen.getByRole('button', { name: /Session actions for/ }),
    );
    fireEvent.click(screen.getByRole('menuitem', { name: 'Close' }));

    await waitFor(() => expect(closed).toHaveBeenCalledWith(item.session.id));
  });
});

describe('SessionNode focus', () => {
  it('focuses the session and activates its main thread when the card header is clicked', () => {
    // Focus is now driven inside the row (previously an `onFocus` prop): the
    // header click focuses the session and selects its main thread — the main
    // thread is not listed in the tree, so the header is how you reach it.
    renderNode({});

    fireEvent.click(screen.getByTestId('session-node'));

    const nav = useNavStore.getState();
    expect(nav.focusedSessionId).toBe(item.session.id);
    expect(nav.activeThreadId).toBe(item.main_thread_id);
  });
});
