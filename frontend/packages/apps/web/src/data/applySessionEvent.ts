import type { QueryClient } from '@tanstack/react-query';
import type { SessionEvent, ThreadId } from '@delta/model';
import {
  invalidateSession,
  invalidateThreadMessages,
  invalidateThreads,
} from '@delta/api-client';
import { useLiveStore } from '../store/liveStore';

/**
 * Route a live `SessionEvent` to the two state homes:
 *
 * - **Query cache** (`@tanstack/react-query`): incremental REST-resource growth.
 *   The `/ws` events do not carry message bodies, so we patch the cache by
 *   invalidating the affected `messages`/`threads` queries, which refetches the
 *   freshly-ingested transcript lines. (When the backend later streams message
 *   payloads, `appendMessage` from `@delta/api-client` can patch in place
 *   without a round-trip — the seam is already in place.)
 * - **Live store** (Zustand): ephemeral UI signals that are not REST resources
 *   — the pending-send FIFO, permission notice, unread badges, external input.
 *
 * `activeThreadId` is needed both to decide which transcript to refetch and to
 * attribute unread/external-input markers.
 */
export function applySessionEvent(
  event: SessionEvent,
  queryClient: QueryClient,
  activeThreadId: ThreadId | null,
): void {
  const store = useLiveStore.getState();

  // Ephemeral signals always go to the store.
  store.applyEvent(event, activeThreadId);

  switch (event.kind) {
    case 'turn_started':
    case 'turn_completed':
      // Transcript grew: refetch the active thread's messages.
      if (activeThreadId !== null) {
        invalidateThreadMessages(queryClient, activeThreadId);
      }
      // A branch send may have created a new thread; keep the tree fresh.
      invalidateThreads(queryClient);
      break;
    case 'external_input':
      // Direct-pane input lands on the last active thread; refetch + badge it.
      if (activeThreadId !== null) {
        invalidateThreadMessages(queryClient, activeThreadId);
        store.bumpUnread(activeThreadId);
      }
      break;
    case 'transcript_updated':
      // The continuous tail ingested new lines (e.g. the assistant reply Claude
      // Code flushed after `Stop`). Pure refetch: invalidate every affected
      // thread plus the active one, with no FIFO/unread mutation.
      for (const threadId of event.thread_ids) {
        invalidateThreadMessages(queryClient, threadId);
      }
      if (
        activeThreadId !== null &&
        !event.thread_ids.includes(activeThreadId)
      ) {
        invalidateThreadMessages(queryClient, activeThreadId);
      }
      invalidateThreads(queryClient);
      break;
    case 'session_registered':
      // The first message just created the session row. Refetch the session
      // query (it was 404/errored during the bootstrap state) so the UI leaves
      // the no-session bootstrap and renders the normal workspace.
      invalidateSession(queryClient);
      invalidateThreads(queryClient);
      break;
    case 'permission_requested':
      // Pure UI notice; already handled by the store.
      break;
  }
}
