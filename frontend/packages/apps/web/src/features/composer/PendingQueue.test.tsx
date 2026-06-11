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
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore, type PendingItem } from '../../store/liveStore';
import { NEW_SESSION_DRAFT_KEY } from '../../store/composerStore';
import { PendingQueue } from './PendingQueue';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderQueue() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <PendingQueue threadId={NEW_SESSION_DRAFT_KEY} />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

const failedSpawn: PendingItem = {
  localId: 'spawn',
  sendId: 0,
  sessionId: null,
  threadId: NEW_SESSION_DRAFT_KEY,
  text: 'start a new session',
  semanticParentUuid: null,
  workdir: '/work/dir',
  status: 'failed',
  createdAt: 0,
};

describe('PendingQueue failed spawn', () => {
  beforeEach(() => {
    useLiveStore.setState({ pending: [] });
  });

  it('renders a failed spawn with an error message plus Retry and Dismiss', () => {
    useLiveStore.setState({ pending: [failedSpawn] });
    renderQueue();

    expect(screen.getByText(/failed to start/i)).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Retry' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Dismiss' }),
    ).toBeInTheDocument();
  });

  it('Dismiss removes the failed chip', () => {
    useLiveStore.setState({ pending: [failedSpawn] });
    renderQueue();

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('Retry drops the failed chip and re-enqueues a new-session send', async () => {
    useLiveStore.setState({ pending: [failedSpawn] });
    renderQueue();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    // The failed chip is removed and a fresh queued new-session pending is
    // enqueued with the same text and chosen directory.
    await waitFor(() => {
      const pending = useLiveStore.getState().pending;
      expect(pending).toHaveLength(1);
      expect(pending[0].localId).not.toBe('spawn');
    });
    const fresh = useLiveStore.getState().pending[0];
    expect(fresh.text).toBe('start a new session');
    expect(fresh.workdir).toBe('/work/dir');
    expect(fresh.sessionId).toBeNull();
    expect(fresh.threadId).toBe(NEW_SESSION_DRAFT_KEY);
    // The mock new-session POST resolves with id 0, attached after the request.
    await waitFor(() => {
      expect(useLiveStore.getState().pending[0].sendId).toBe(0);
    });
  });
});
