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
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
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
      pending: [],
      externalInput: {},
      unread: {},
      resumeUnavailable: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
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

    // The optimistic FIFO entry confirms the send fired; the active thread must
    // stay on main.
    await waitFor(() => {
      expect(useLiveStore.getState().pending.length).toBe(1);
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

    // A resume send queues optimistically against the active thread (here main).
    await waitFor(() => {
      expect(useLiveStore.getState().pending.length).toBe(1);
    });
    expect(useLiveStore.getState().pending[0]?.threadId).toBe(MAIN_THREAD_ID);
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
      expect(useLiveStore.getState().pending.length).toBe(1);
    });
    expect(useLiveStore.getState().pending[0]?.threadId).toBe(BRANCH_THREAD_ID);
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

    // The optimistic entry is marked failed when the resume request errors.
    await waitFor(() => {
      const pending = useLiveStore.getState().pending;
      expect(pending[0]?.status).toBe('failed');
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
        useLiveStore.getState().resumeUnavailable[mainThread.session_id],
      ).toBe(true);
    });
    // No lingering chip: a resume-unavailable send is removed outright, not
    // marked failed.
    expect(useLiveStore.getState().pending).toHaveLength(0);
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

    // The optimistic send is enqueued; the synthetic id:0 attaches on success.
    await waitFor(() => {
      const pending = useLiveStore.getState().pending;
      expect(pending.length).toBe(1);
      expect(pending[0].text).toBe('start fresh');
    });
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
    // A successful new-session send resets the picker selection.
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
    });
  });
});
