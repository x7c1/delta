import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import {
  MAIN_THREAD_ID,
  createHandlers,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { TranscriptPane } from './TranscriptPane';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPane() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const main = mockThreads.find((t) => t.id === MAIN_THREAD_ID)!;
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane threads={mockThreads} activeThread={main} />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('TranscriptPane', () => {
  beforeEach(() => {
    useNavStore.setState({ activeThreadId: MAIN_THREAD_ID });
    useLiveStore.setState({ pending: [], externalInput: null });
    useComposerStore.setState({ drafts: {}, branchOrigin: null });
  });

  it('renders messages fetched from the mocked REST API', async () => {
    renderPane();

    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );
    // Assistant Markdown text is foregrounded.
    expect(screen.getByText(/change between two states/)).toBeInTheDocument();
    // The breadcrumb shows the current location.
    expect(
      screen.getByRole('navigation', { name: 'Breadcrumb' }),
    ).toHaveTextContent('main');
  });

  it('renders a branch chip where a child thread sprouts', async () => {
    renderPane();

    await waitFor(() =>
      expect(screen.getByText(/delta etymology/)).toBeInTheDocument(),
    );
  });
});
