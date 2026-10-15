/**
 * Shared fixtures for the `NavigatorPane.*.test.tsx` family: the MSW server,
 * the session-list item builder, and the render helper.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import { render, type RenderResult } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import {
  createHandlers,
  SESSION_ID,
  SESSION_ID_2,
  SESSION_2_MAIN_THREAD_ID,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { AgentProvider, SessionListItem } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { NavigatorPane } from './NavigatorPane';

export const server = setupServer(...createHandlers());

export function makeItem(
  id: string,
  mainThreadId: number,
  provider: AgentProvider = 'claude',
): SessionListItem {
  return {
    session: {
      id,
      cwd: `/home/dev/${id}`,
      transcript_path: '',
      title: null,
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
      branch_at_launch: 'main',
      repo_root: `/home/dev/${id}`,
      repository_display_name: `dev/${id}`,
      provider,
      provider_session_id: null,
      provider_thread_id: null,
      pull_request_number: null,
      failure_reason: null,
      closed_reason: null,
    },
    open: true,
    pane_starting: false,
    hooks_unreachable: false,
    main_thread_id: mainThreadId,
    threads: [
      {
        id: mainThreadId,
        session_id: id,
        title: 'main',
        parent_thread_id: null,
        root_message_uuid: null,
        created_at: '2026-01-01T00:00:00Z',
        last_activity_at: null,
      },
    ],
    last_activity_at: '2026-01-01T00:00:00Z',
  };
}

const sessions = [
  makeItem(SESSION_ID, 1),
  makeItem(SESSION_ID_2, SESSION_2_MAIN_THREAD_ID),
];

export function renderPane(items: SessionListItem[] = sessions): RenderResult {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <NavigatorPane
          sessions={items}
          hasMoreSessions={false}
          isLoadingMoreSessions={false}
          onLoadMoreSessions={() => {}}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}
