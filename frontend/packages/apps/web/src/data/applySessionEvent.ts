import type { QueryClient } from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type { SessionEvent } from '@delta/wire-gen';
import {
  invalidateSessions,
  invalidateSessionThreads,
  invalidateThreadMessages,
} from '@delta/api-client';
import { useLiveStore } from '../store/liveStore';

/**
 * Route a live `SessionEvent` to the two state homes:
 *
 * - **Query cache** (`@tanstack/react-query`): incremental REST-resource growth.
 *   The `/ws` events do not carry message bodies, so we patch the cache by
 *   invalidating the affected `messages`/`session-threads` queries, which
 *   refetches the freshly-ingested transcript lines. Lifecycle events
 *   (`session_registered`/`session_opened`/`session_closed`) invalidate the
 *   session list so a newly-spawned, resumed, or closed session's presence and
 *   open flag stay in sync.
 * - **Live store** (Zustand): ephemeral UI signals that are not REST resources
 *   — the pending-send FIFO, permission notice, unread badges, external input,
 *   and the per-session resuming marker.
 *
 * Transcript/turn events are scoped to the focused session: `activeThreadId`
 * selects which transcript to refetch and which thread to badge, and
 * `focusedSessionId` selects whose thread tree to refresh. Events for a
 * non-focused session still refresh the session list but never touch the
 * focused transcript.
 */
export function applySessionEvent(
  event: SessionEvent,
  queryClient: QueryClient,
  activeThreadId: ThreadId | null,
  focusedSessionId: SessionId | null,
): void {
  const store = useLiveStore.getState();

  // Session-scoped ephemeral signals (pending FIFO, permission, resuming) always
  // go to the store; focus-dependent signals are handled below under a guard.
  store.applyEvent(event);

  const isFocused = focusedSessionId !== null && event.session_id === focusedSessionId;

  const refreshFocusedThreads = () => {
    if (focusedSessionId !== null) {
      invalidateSessionThreads(queryClient, focusedSessionId);
    }
  };

  switch (event.kind) {
    case 'turn_started':
    case 'turn_completed':
    case 'turn_interrupted':
      // Transcript grew on the focused session: refetch the active thread. An
      // interrupt also appends the `[Request interrupted by user]` marker line,
      // so it refetches the same way a completed turn does.
      if (isFocused && activeThreadId !== null) {
        invalidateThreadMessages(queryClient, activeThreadId);
      }
      // A branch send may have created a new thread; keep the tree fresh.
      if (isFocused) {
        refreshFocusedThreads();
      }
      break;
    case 'external_input':
      // Direct-pane input lands on the focused session's active thread. The
      // marker is recorded only for the focused session so a background
      // session's typing never surfaces on the transcript the user is viewing.
      if (isFocused && focusedSessionId !== null && activeThreadId !== null) {
        invalidateThreadMessages(queryClient, activeThreadId);
        store.bumpUnread(activeThreadId);
        store.noteExternalInput(focusedSessionId, activeThreadId, event.prompt);
      }
      break;
    case 'transcript_updated':
      // The continuous tail ingested new lines. Pure refetch: invalidate every
      // affected thread plus the focused active one, with no FIFO/unread change.
      for (const threadId of event.thread_ids) {
        invalidateThreadMessages(queryClient, threadId);
      }
      if (
        isFocused &&
        activeThreadId !== null &&
        !event.thread_ids.includes(activeThreadId)
      ) {
        invalidateThreadMessages(queryClient, activeThreadId);
      }
      if (isFocused) {
        refreshFocusedThreads();
      }
      break;
    case 'session_registered':
    case 'session_opened':
    case 'session_closed':
      // A session was spawned/bound, resumed, or closed. Refresh the session
      // list so its presence and open flag update, and refresh the focused
      // session's threads if it is the one affected.
      invalidateSessions(queryClient);
      if (isFocused) {
        refreshFocusedThreads();
      }
      break;
    case 'permission_requested':
    case 'permission_resolved':
      // Pure UI notice (set/cleared); already handled by the store.
      break;
    case 'spawn_failed':
      // A freshly-spawned session never bound. Mark the optimistic new-session
      // chip `failed` so it stops looking stuck and offers Retry / Dismiss. No
      // session-list refetch: the spawn never registered, so the list never
      // gained (and so cannot lose) a row for it.
      store.failSpawn();
      break;
  }
}
