/**
 * Shared fixtures for the `SendActivityStrip.*.test.tsx` family: the MSW
 * server, the strip as the transcript pane mounts it, the per-case store reset
 * and a server send builder.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob. Each test file registers the server's lifecycle.
 */
import { render, type RenderResult } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers, SESSION_ID } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { Send } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { useNotificationStore } from '../../store/notificationStore';
import { SendActivityStrip } from './SendActivityStrip';
import { useSendActivity, type SendActivitySurface } from './useSendActivity';

export const server = setupServer(...createHandlers());

/** The strip as the transcript pane mounts it: rows merged per surface. */
function Strip({
  surface,
  sessionSpawning,
  providerHasTerminal,
}: {
  surface: SendActivitySurface;
  sessionSpawning?: boolean;
  providerHasTerminal?: boolean;
}) {
  const entries = useSendActivity(surface);
  return (
    <SendActivityStrip
      entries={entries}
      sessionSpawning={sessionSpawning}
      sessionId={surface.kind === 'thread' ? surface.sessionId : null}
      providerHasTerminal={providerHasTerminal}
    />
  );
}

export function renderStrip(
  surface: SendActivitySurface,
  seed?: (queryClient: QueryClient) => void,
  // As the transcript pane relays it: the focused session is still starting.
  sessionSpawning?: boolean,
  // As the transcript pane relays it: the session's provider has a terminal.
  providerHasTerminal?: boolean,
): RenderResult {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  seed?.(queryClient);
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <Strip
          surface={surface}
          sessionSpawning={sessionSpawning}
          providerHasTerminal={providerHasTerminal}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

export function reset() {
  useLiveStore.setState({
    sending: [],
    localSends: {},
    spawns: [],
    runningThreads: {},
    notices: {},
  });
  useNotificationStore.setState({ notifications: [] });
}

export function serverSend(overrides: Partial<Send> = {}): Send {
  return {
    id: 1,
    session_id: SESSION_ID,
    thread_id: 1,
    semantic_parent_uuid: null,
    text: 'a send',
    locator_quote: null,
    status: 'dispatched',
    matched_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    held_at: null,
    ...overrides,
  };
}
