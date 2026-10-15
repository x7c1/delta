/**
 * Sending into a thread: branch and plain sends, starting and lost-contact
 * sessions, and resuming a closed one.
 *
 * Shared fixtures live in Composer.testkit.tsx.
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
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { Composer } from './Composer';
import {
  mainThread,
  renderComposer,
  resetComposerState,
  server,
} from './Composer.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('Composer', () => {
  beforeEach(resetComposerState);

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

  it('still offers send while the session is still starting', async () => {
    // A session is focusable from the moment its first send is accepted, so
    // its composer is reachable while the launch is still coming up. The
    // server accepts a plain send there as a `queued` row and types it when
    // the launch binds, so Send stays live — holding the user at a disabled
    // button through a minute-long checkout would withhold something that
    // works. Only the placeholder changes, to say the message waits.
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
              // A starting session has no live pane either, so it arrives
              // read-only; `spawning` is what changes what the composer says.
              readOnly: true,
              spawning: true,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    expect(textarea).toHaveAttribute(
      'placeholder',
      'Message sends when the session is ready…',
    );

    // An empty draft is still no send; typing one arms the button.
    const send = screen.getByRole('button', { name: 'Send' });
    expect(send).toBeDisabled();
    fireEvent.change(textarea, { target: { value: 'while it starts' } });
    expect(send).toBeEnabled();

    fireEvent.click(send);

    // The send goes out and the server's `queued` row is tracked like any
    // other accepted send.
    await waitFor(() => {
      expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);
    });
    expect(useLiveStore.getState().sending).toHaveLength(0);
  });

  it('refuses to send into a session that lost contact with its hooks', async () => {
    // A session re-adopted after a restart whose agent's hooks no longer reach
    // the server: typing into its pane would make the server interrupt the
    // running turn and submit the prompt twice, so the server refuses the
    // send. The composer does not even try — it points at the lost-contact
    // notice above and keeps the draft for after a Close.
    let posted = 0;
    server.use(
      http.post('http://localhost/api/sends', () => {
        posted += 1;
        return HttpResponse.json(
          { error: 'unreachable', code: 'session_hooks_unreachable' },
          { status: 409 },
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
          <Composer
            mode={{
              kind: 'thread',
              activeThread: mainThread,
              readOnly: false,
              spawning: false,
              hooksUnreachable: true,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    expect(textarea).toHaveAttribute(
      'placeholder',
      'Delta lost contact with this session — see the notice above…',
    );
    fireEvent.change(textarea, { target: { value: 'keep going' } });
    const send = screen.getByRole('button', { name: 'Send' });
    expect(send).toBeDisabled();
    expect(send).toHaveAttribute(
      'title',
      'Delta lost contact with this session — see the notice above',
    );

    // Neither the button nor the keyboard shortcut sends.
    fireEvent.click(send);
    fireEvent.keyDown(textarea, { key: 'Enter', ctrlKey: true });
    await act(async () => {
      await Promise.resolve();
    });
    expect(posted).toBe(0);
    expect(useLiveStore.getState().sending).toHaveLength(0);
    expect(useComposerStore.getState().drafts[MAIN_THREAD_ID]).toBe(
      'keep going',
    );
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
              spawning: false,
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
            mode={{
              kind: 'thread',
              activeThread: subThread,
              readOnly: true,
              spawning: false,
            }}
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
              spawning: false,
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
              spawning: false,
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
              spawning: false,
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
});
