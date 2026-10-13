/**
 * Shared fixtures for the `applySessionEvent.*.test.ts` family: the
 * focused-session constant, the send and session-list builders, and the
 * per-case store reset.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import { QueryClient } from '@tanstack/react-query';
import { queryKeys } from '@delta/api-client';
import type { Send, SessionListItem } from '@delta/wire-gen';
import { useLiveStore } from '../store/liveStore';
import { useNavStore } from '../store/navStore';
import { useNotificationStore } from '../store/notificationStore';

export const FOCUSED = 'sess-1';

export function serverSend(overrides: Partial<Send> = {}): Send {
  return {
    id: 1,
    session_id: FOCUSED,
    thread_id: 5,
    semantic_parent_uuid: null,
    text: 'hi',
    locator_quote: null,
    status: 'dispatched',
    matched_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    held_at: null,
    ...overrides,
  };
}

/**
 * Seed the session-list cache with `ids`, in list order, as the infinite query
 * stores it. The removal handler reads the head of this cache to decide where
 * focus goes, so the order here is the order the navigator would show.
 */
export function seedSessionList(queryClient: QueryClient, ids: readonly string[]) {
  const sessions: SessionListItem[] = ids.map((id) => ({
    session: {
      id,
      cwd: '/work',
      transcript_path: '',
      title: null,
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
      branch_at_launch: null,
      repo_root: null,
      repository_display_name: null,
      provider: 'claude',
      provider_session_id: null,
      provider_thread_id: null,
      pull_request_number: null,
      failure_reason: null,
    },
    open: false,
    pane_starting: false,
    hooks_unreachable: false,
    main_thread_id: 1,
    threads: [],
    last_activity_at: null,
  }));
  queryClient.setQueryData(queryKeys.sessions, {
    pages: [{ sessions, next_cursor: null }],
    pageParams: [null],
  });
}

/** Reset the stores every case starts from. */
export function resetApplySessionEventState(): void {
  useLiveStore.setState({
    connection: 'connecting',
    sending: [],
    localSends: {},
    spawns: [],
    runningThreads: {},
    runningSubagents: {},
    notices: {},
    resolvedRequests: {},
    unread: {},
    threadActivity: {},
    streamingMessages: {},
  });
  useNavStore.setState({ focusedSessionId: FOCUSED });
  // A spawn failure this client never tracked announces itself in the
  // app-wide snackbar, which is shared state these tests move too.
  useNotificationStore.setState({ notifications: [] });
}
