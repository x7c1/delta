/**
 * Shared fixtures for the `Composer.*.test.tsx` family: the MSW server, the
 * main thread, the render helper, and the per-case store reset.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import { render, type RenderResult } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { MAIN_THREAD_ID, createHandlers, mockThreads } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { Composer } from './Composer';

export const server = setupServer(...createHandlers());

export const mainThread = mockThreads.find((t) => t.id === MAIN_THREAD_ID) as Thread;

export function renderComposer(
  activeThread: Thread = mainThread,
): RenderResult {
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
            spawning: false,
          }}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/** Reset the stores every case starts from. */
export function resetComposerState(): void {
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
    newSessionProvider: 'claude',
  });
}
