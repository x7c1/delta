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
import { fireEvent, render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers, SESSION_ID } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SessionListItem } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { SessionNode } from './SessionNode';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => {
  server.resetHandlers();
  // Running and unread are read from the live store (thread-keyed), OR-aggregated
  // over the session's threads onto the collapsed row — reset between cases.
  useLiveStore.setState({ runningThreads: {}, unread: {}, runningSubagents: {} });
});
afterAll(() => server.close());

/** Flag the session's main thread (id 1) running in the store. */
function setRunning() {
  useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 1: true } } });
}

/** Flag the session's main thread (id 1) unread in the store. */
function setUnread() {
  useLiveStore.setState({ unread: { 1: 1 } });
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
  },
  open: true,
  main_thread_id: 1,
  last_activity_at: '2026-01-01T00:00:00Z',
};

function renderNode(props: Partial<ComponentProps<typeof SessionNode>>) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <SessionNode
          item={item}
          isFocused={false}
          onFocus={() => {}}
          onClose={() => {}}
          {...props}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('SessionNode running indicator', () => {
  it('renders the running indicator when a thread of the session is running', () => {
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

  it('shows the running indicator and the permission badge together', () => {
    setRunning();
    renderNode({ needsPermission: true });

    expect(screen.getByTestId('session-running')).toBeInTheDocument();
    expect(
      screen.getByTestId('session-permission-badge'),
    ).toBeInTheDocument();
  });

  it('shows the running indicator when only a subagent is running (turn idle)', () => {
    // The launching turn has ended but its background subagent keeps working;
    // the thread still reads as running so the row shows the spinner.
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
});
