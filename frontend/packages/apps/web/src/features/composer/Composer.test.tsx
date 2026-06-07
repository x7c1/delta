import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { MAIN_THREAD_ID, createHandlers, mockThreads } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { Thread } from '@delta/model';
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
            sessionMainThreadId: MAIN_THREAD_ID,
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
      externalInput: null,
      unread: {},
      resuming: {},
    });
    useComposerStore.setState({ drafts: {}, branchOrigin: null });
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

  it('marks the session resuming on a closed (read-only) send', async () => {
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
              sessionMainThreadId: MAIN_THREAD_ID,
            }}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume please' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(useLiveStore.getState().resuming[mainThread.session_id]).toBe(true);
    });
  });

  it('targets a new session when in new-session mode', async () => {
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
});
