import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import type { MessagesResponse } from '@delta/model';
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

function renderPane(threads = mockThreads) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const main = threads.find((t) => t.id === MAIN_THREAD_ID)!;
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane threads={threads} activeThread={main} readOnly={false} />
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

  it('hides the breadcrumb until the session has branched', async () => {
    // A main-only session (no sub-threads) should not show a lone "main"
    // breadcrumb, which reads as abrupt with no tree to place it in.
    const mainOnly = mockThreads.filter((t) => t.parent_thread_id === null);
    renderPane(mainOnly);

    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole('navigation', { name: 'Breadcrumb' }),
    ).not.toBeInTheDocument();
  });

  it('renders a branch chip where a child thread sprouts', async () => {
    renderPane();

    await waitFor(() =>
      expect(screen.getByText(/delta etymology/)).toBeInTheDocument(),
    );
  });

  it('does not render non-conversational (system/other) lines', async () => {
    // The transcript persists these lines but the view must skip them.
    server.use(
      http.get('*/api/threads/:id/messages', () => {
        const body: MessagesResponse = {
          messages: [
            {
              uuid: 'm-user',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'user',
              linear_parent_uuid: null,
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 0,
              content_text: 'hello there',
              content: [{ type: 'text', text: 'hello there' }],
              created_at: '2026-01-01T00:00:01Z',
            },
            {
              uuid: 'm-system',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'system',
              linear_parent_uuid: 'm-user',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: 'SECRET SYSTEM NOISE',
              content: [{ type: 'text', text: 'SECRET SYSTEM NOISE' }],
              created_at: '2026-01-01T00:00:02Z',
            },
            {
              uuid: 'm-other',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'other',
              linear_parent_uuid: 'm-system',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 2,
              content_text: 'OTHER NOISE',
              content: [{ type: 'text', text: 'OTHER NOISE' }],
              created_at: '2026-01-01T00:00:03Z',
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane();

    await waitFor(() =>
      expect(screen.getByText('hello there')).toBeInTheDocument(),
    );
    expect(screen.queryByText('SECRET SYSTEM NOISE')).not.toBeInTheDocument();
    expect(screen.queryByText('OTHER NOISE')).not.toBeInTheDocument();
  });
});
