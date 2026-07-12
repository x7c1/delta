import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  createHandlers,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { COMPOSER_MAX_HEIGHT, COMPOSER_MIN_HEIGHT } from './autoGrow';
import { Composer } from './Composer';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const mainThread = mockThreads.find((t) => t.id === MAIN_THREAD_ID) as Thread;

function renderComposer(activeThread: Thread = mainThread) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <Composer
          mode={{
            kind: 'thread',
            activeThread,
            readOnly: false,
          }}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('Composer', () => {
  beforeEach(() => {
    useNavStore.setState({ activeThreadId: MAIN_THREAD_ID });
    useLiveStore.setState({
      sending: [],
      localSends: {},
      spawns: [],
      notices: {},
      unread: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      newSessionLaunchOptionIds: [],
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: { kind: 'head' },
    });
  });

  it('switches the active thread to the new child after a branch send', async () => {
    // A branch origin on the active (main) thread turns the send into a branch.
    useComposerStore.setState({
      branchOrigin: {
        parentThreadId: MAIN_THREAD_ID,
        semanticParentUuid: 'uuid-a',
        locatorQuote: 'selected text',
      },
    });
    renderComposer();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'follow-up question' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // The mock creates a fresh child thread and returns its id on the send;
    // the composer must drill into it and clear the branch origin.
    await waitFor(() => {
      const active = useNavStore.getState().activeThreadId;
      expect(active).not.toBeNull();
      expect(active).not.toBe(MAIN_THREAD_ID);
    });
    expect(useComposerStore.getState().branchOrigin).toBeNull();
  });

  it('keeps the active thread on a plain (non-branch) send', async () => {
    renderComposer();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'plain message' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // The accepted send is tracked locally (chip continuity through its turn);
    // the active thread must stay on main.
    await waitFor(() => {
      expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);
    });
    expect(useNavStore.getState().activeThreadId).toBe(MAIN_THREAD_ID);
  });

  it('enqueues an optimistic send on a closed (read-only) resume', async () => {
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer
            mode={{
              kind: 'thread',
              activeThread: mainThread,
              readOnly: true,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume please' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // A resume send is accepted and tracked against the active thread (main).
    await waitFor(() => {
      expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);
    });
    expect(Object.values(useLiveStore.getState().localSends)[0]?.threadId).toBe(
      MAIN_THREAD_ID,
    );
  });

  it('resumes a closed session onto the active SUB-thread, not main', async () => {
    // Regression: continuing a closed session from one of its sub-threads must
    // stay on that sub-thread. The send targets the active thread (the backend
    // resumes the session), so the optimistic entry keys to the sub-thread —
    // not the session's main thread.
    const subThread = mockThreads.find(
      (t) => t.id === BRANCH_THREAD_ID,
    ) as Thread;
    useNavStore.setState({ activeThreadId: BRANCH_THREAD_ID });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer
            mode={{ kind: 'thread', activeThread: subThread, readOnly: true }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'continue the sub-thread' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);
    });
    expect(Object.values(useLiveStore.getState().localSends)[0]?.threadId).toBe(
      BRANCH_THREAD_ID,
    );
  });

  it('branches from a closed (read-only) session, drilling into the new child', async () => {
    // A quote selected in a closed session sets a branch origin. The send must
    // still branch (not degrade into a plain resume): the backend resumes the
    // session and creates the child, and the composer drills into it.
    useComposerStore.setState({
      branchOrigin: {
        parentThreadId: MAIN_THREAD_ID,
        semanticParentUuid: 'uuid-a',
        locatorQuote: 'old passage',
      },
    });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer
            mode={{
              kind: 'thread',
              activeThread: mainThread,
              readOnly: true,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'dig into this' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      const active = useNavStore.getState().activeThreadId;
      expect(active).not.toBeNull();
      expect(active).not.toBe(MAIN_THREAD_ID);
    });
    expect(useComposerStore.getState().branchOrigin).toBeNull();
  });

  it('marks the optimistic send failed when the resume request fails', async () => {
    // Force the send to fail so the resume never starts.
    server.use(
      http.post('*/api/sends', () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 }),
      ),
    );

    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer
            mode={{
              kind: 'thread',
              activeThread: mainThread,
              readOnly: true,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume please' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // The submit chip is marked failed when the resume request errors.
    await waitFor(() => {
      const sending = useLiveStore.getState().sending;
      expect(sending[0]?.status).toBe('failed');
    });
  });

  it('drops the optimistic send and flags the session when resume is unavailable', async () => {
    // The server refuses the resume because the transcript is gone (409 with a
    // stable code). Unlike a generic failure, the optimistic chip must be
    // dropped entirely (not left "failed") and the session flagged so the inline
    // notice can show — the session stays closed.
    server.use(
      http.post('*/api/sends', () =>
        HttpResponse.json(
          { error: 'session cannot be resumed', code: 'resume_unavailable' },
          { status: 409 },
        ),
      ),
    );

    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer
            mode={{
              kind: 'thread',
              activeThread: mainThread,
              readOnly: true,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume please' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(
        noticeOf(
          useLiveStore.getState().notices,
          mainThread.session_id,
          'resume_unavailable',
        ),
      ).not.toBeNull();
    });
    // No lingering chip: a resume-unavailable send is removed outright, not
    // marked failed.
    expect(useLiveStore.getState().sending).toHaveLength(0);
  });

  it('targets a new session when in new-session mode', async () => {
    // A new session needs a chosen directory before Send is enabled.
    useComposerStore.setState({ newSessionWorkdir: '/home/dev' });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // The accepted spawn is tracked under the REAL ids the server returned, so
    // the workspace can focus the new session directly and the chip survives
    // through the first turn.
    await waitFor(() => {
      const spawns = useLiveStore.getState().spawns;
      expect(spawns).toHaveLength(1);
      expect(spawns[0].text).toBe('start fresh');
      expect(spawns[0].sessionId).not.toBe('');
      expect(spawns[0].status).toBe('spawning');
    });
    const locals = Object.values(useLiveStore.getState().localSends);
    expect(locals).toHaveLength(1);
    expect(locals[0].text).toBe('start fresh');
  });

  it('disables Send while the worktree toggle is on but no branch is picked yet', async () => {
    // Dogfooding default: toggling worktree ON lands on the Other-remote-
    // branch picker (the `pending_remote_branch` sentinel) so the user picks
    // a specific branch. The backend rejects a worktree request without a
    // concrete branch, so Send must stay disabled until the user picks one
    // — picking unblocks Send.
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'pending_remote_branch' },
    });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();

    // Picking a concrete branch unblocks Send.
    act(() => {
      useComposerStore.setState({
        newSessionWorktreeStartPoint: {
          kind: 'use_remote_branch',
          name: 'develop',
        },
      });
    });
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
    });
  });

  it('Cmd+Enter is a no-op for a new session when no workdir is selected', async () => {
    // Regression: the Send button gates a new session on `newSessionWorkdir`,
    // but the keyboard shortcut used to bypass that gate. Without a workdir
    // the backend would spawn in its default scratch dir and the watchdog
    // would later report "session failed to start", so Cmd+Enter must obey
    // the same disabled rule as the button.
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    fireEvent.keyDown(textarea, { key: 'Enter', metaKey: true });
    fireEvent.keyDown(textarea, { key: 'Enter', ctrlKey: true });

    // No spawn or local send was queued — the shortcut silently did nothing,
    // matching the disabled Send button.
    expect(useLiveStore.getState().spawns).toHaveLength(0);
    expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(0);
  });

  it('Cmd+Enter submits a new session once a workdir is selected', async () => {
    // Pair to the guard test above: once the gate is satisfied, the shortcut
    // must still work (we are guarding, not removing it).
    useComposerStore.setState({ newSessionWorkdir: '/home/dev' });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    fireEvent.keyDown(textarea, { key: 'Enter', metaKey: true });

    await waitFor(() => {
      expect(useLiveStore.getState().spawns).toHaveLength(1);
    });
    expect(useLiveStore.getState().spawns[0].text).toBe('start fresh');
  });

  it('disables Send for a new session until a workdir is selected', async () => {
    // Default state: no directory chosen. Selection is mandatory for a new
    // session, so Send stays disabled even with text entered.
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();

    // Once a directory is selected, Send becomes enabled.
    act(() => {
      useComposerStore.setState({ newSessionWorkdir: '/home/dev' });
    });
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
    });
  });

  /**
   * Render the new-session composer and capture the body of the `POST
   * /api/sends` it fires, so a test can assert exactly which fields are sent.
   */
  function renderNewSessionAndCaptureBody(): { read: () => unknown } {
    let captured: unknown;
    server.use(
      http.post('*/api/sends', async ({ request }) => {
        captured = await request.json();
        return HttpResponse.json(
          {
            send: {
              id: 0,
              session_id: '',
              thread_id: 0,
              semantic_parent_uuid: null,
              text: 'irrelevant',
              locator_quote: null,
              status: 'dispatched',
              matched_uuid: null,
              created_at: '2026-01-01T00:00:00Z',
            },
          },
          { status: 201 },
        );
      }),
    );
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );
    return { read: () => captured };
  }

  it('wires the auto-grow effect: an inline height and overflow style are applied', async () => {
    // jsdom performs no layout, so `scrollHeight` is 0 and the clamp resolves to
    // the min height; we cannot assert real growth here (covered by the
    // autoGrow.test.ts unit test and the stubbed-scrollHeight tests below). What
    // we CAN assert is that the effect ran and drove the textarea's inline
    // geometry — the wiring that makes it auto-grow in a real browser. The
    // measurement is coalesced into an animation frame, so wait for it.
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: 'one\ntwo\nthree' } });
    // The effect set an explicit pixel height and toggled the internal scrollbar.
    await waitFor(() => expect(textarea.style.height).toMatch(/px$/));
    // Below the cap (0 scrollHeight in jsdom) the bar stays hidden.
    expect(textarea.style.overflowY).toBe('hidden');
    // The manual resize handle is gone (auto-grow replaces it).
    expect(textarea.className).toContain('resize-none');
    expect(textarea.className).not.toContain('resize-y');
  });

  it('resets the textarea height after a submit clears the draft', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: 'plain message' } });
    expect(textarea.value).toBe('plain message');

    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // Submit clears the draft; the controlled value empties and the auto-grow
    // effect re-runs (coalesced into a frame), leaving the textarea reset to its
    // (min) height with the scrollbar hidden.
    await waitFor(() => expect(textarea.value).toBe(''));
    await waitFor(() => expect(textarea.style.height).toMatch(/px$/));
    expect(textarea.style.overflowY).toBe('hidden');
  });

  // jsdom performs no layout, so `scrollHeight` is always 0 there; model a real
  // browser's content-height report by deriving it from the current value's line
  // count (each line ~20px over an 8px base). This lets the JS-driven autosize —
  // the part that stays in JS after the perf change — be exercised end to end:
  // it reads `scrollHeight` inside its coalesced frame, clamps via
  // `autoGrowGeometry`, and writes the inline height + overflow.
  function stubScrollHeight(textarea: HTMLTextAreaElement): void {
    const base = 8;
    const perLine = 20;
    Object.defineProperty(textarea, 'scrollHeight', {
      configurable: true,
      get(this: HTMLTextAreaElement) {
        const lines = (this.value.match(/\n/g)?.length ?? 0) + 1;
        return base + lines * perLine;
      },
    });
  }

  it('grows the textarea as multi-line content is typed', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    // One short line stays at the min height (content shorter than the min).
    fireEvent.change(textarea, { target: { value: 'one' } });
    await waitFor(() =>
      expect(textarea.style.height).toBe(`${COMPOSER_MIN_HEIGHT}px`),
    );

    // Three lines (8 + 3*20 = 68px) grow past the min, below the cap.
    fireEvent.change(textarea, { target: { value: 'one\ntwo\nthree' } });
    await waitFor(() => expect(textarea.style.height).toBe('68px'));
    expect(textarea.style.overflowY).toBe('hidden');
  });

  it('shrinks the textarea back when content is deleted', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    fireEvent.change(textarea, { target: { value: 'a\nb\nc\nd\ne' } });
    await waitFor(() => expect(textarea.style.height).toBe('108px')); // 8 + 5*20

    // Deleting back down to two lines shrinks the height again (auto-grow resets
    // to `auto` before measuring, so it never sticks at the taller value).
    fireEvent.change(textarea, { target: { value: 'a\nb' } });
    await waitFor(() => expect(textarea.style.height).toBe('48px')); // 8 + 2*20
    expect(textarea.style.overflowY).toBe('hidden');
  });

  it('clamps at the max height and switches to internal scrolling', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    // Ten lines (8 + 10*20 = 208px) exceed the 160px cap: the height is clamped
    // and the internal scrollbar is enabled.
    fireEvent.change(textarea, {
      target: { value: Array.from({ length: 10 }, (_, i) => `l${i}`).join('\n') },
    });
    await waitFor(() =>
      expect(textarea.style.height).toBe(`${COMPOSER_MAX_HEIGHT}px`),
    );
    expect(textarea.style.overflowY).toBe('auto');
  });

  it('resizes after a programmatic draft change (quote insertion / thread switch)', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    // A programmatic draft change — quote insertion or draft restore on a thread
    // switch — mutates the store rather than firing an input event. The textarea
    // must still resize to fit the injected content.
    act(() => {
      useComposerStore.setState({
        drafts: { [MAIN_THREAD_ID]: 'quoted\nline\nhere' },
      });
    });
    await waitFor(() => expect(textarea.value).toBe('quoted\nline\nhere'));
    await waitFor(() => expect(textarea.style.height).toBe('68px')); // 8 + 3*20
  });

  it('includes the selected workdir on a new-session send', async () => {
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start in delta' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start in delta',
        workdir: '/home/dev/projects/delta',
      });
    });
    // The picker selection is preserved after submit; TranscriptPane resets it
    // when the new-session state is left.
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
  });

  it('includes the selected launch options on a new-session send, in order', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      // Selection order (not ascending) so the test pins order preservation.
      newSessionLaunchOptionIds: [3, 1],
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start with options' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start with options',
        workdir: '/home/dev/projects/delta',
        launch_option_ids: [3, 1],
      });
    });
    // The picker selection is preserved after submit; TranscriptPane resets it
    // when the new-session state is left.
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
    expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
      3, 1,
    ]);
  });

  it('includes worktree with the chosen start-point when the toggle is on', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'remote_branch', name: 'develop' },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start in a worktree' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start in a worktree',
        workdir: '/home/dev/projects/delta',
        worktree: { start_point: { kind: 'remote_branch', name: 'develop' } },
      });
    });
  });

  it('includes the use_remote_branch start-point when "use this branch" is chosen', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: {
        kind: 'use_remote_branch',
        name: 'develop',
      },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'work on develop directly' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'work on develop directly',
        workdir: '/home/dev/projects/delta',
        worktree: {
          start_point: { kind: 'use_remote_branch', name: 'develop' },
        },
      });
    });
  });

  it('omits worktree when the toggle is off', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: false,
      // A non-default start-point must NOT leak when the toggle is off.
      newSessionWorktreeStartPoint: { kind: 'remote_branch', name: 'develop' },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'no worktree' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'no worktree',
        workdir: '/home/dev/projects/delta',
      });
    });
  });

  it('omits launch_option_ids when no launch options are selected', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionLaunchOptionIds: [],
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'no options' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'no options',
        workdir: '/home/dev/projects/delta',
      });
    });
  });
});
