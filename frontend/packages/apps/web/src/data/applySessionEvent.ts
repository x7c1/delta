import type { QueryClient } from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type { SessionEvent } from '@delta/wire-gen';
import {
  invalidateSessions,
  invalidateSessionSends,
  invalidateSessionThreads,
  invalidateThreadMessages,
  removeSessionSends,
} from '@delta/api-client';
import { useLiveStore } from '../store/liveStore';

/**
 * Route a live `SessionEvent` to the two state homes:
 *
 * - **Query cache** (`@tanstack/react-query`): incremental REST-resource growth.
 *   The `/ws` events do not carry message bodies, so we patch the cache by
 *   invalidating the affected `messages`/`session-threads` queries, which
 *   refetches the freshly-ingested transcript lines. Send-affecting events
 *   (the turn lifecycle, transcript growth, a close) also invalidate the
 *   session's open-send list — the server-side truth behind the pending strip.
 *   Lifecycle events (`session_registered`/`session_opened`/`session_closed`)
 *   invalidate the session list so a newly-spawned, resumed, or closed
 *   session's presence and open flag stay in sync.
 * - **Live store** (Zustand): ephemeral UI signals that are not REST resources
 *   — turn tracking, the spawn registry, permission notices, unread badges,
 *   external input, and the per-session resuming marker.
 *
 * Transcript/turn events are scoped to the focused session: `activeThreadId`
 * selects which transcript to refetch and which thread to badge, and
 * `focusedSessionId` selects whose thread tree to refresh. Events for a
 * non-focused session still refresh the session list and that session's
 * open-send list, but never touch the focused transcript.
 */
export function applySessionEvent(
  event: SessionEvent,
  queryClient: QueryClient,
  activeThreadId: ThreadId | null,
  focusedSessionId: SessionId | null,
): void {
  const store = useLiveStore.getState();

  // Session-scoped ephemeral signals (turn tracking, spawn registry,
  // permission) always go to the store; focus-dependent signals are handled
  // below under a guard.
  store.applyEvent(event);

  const isFocused = focusedSessionId !== null && event.session_id === focusedSessionId;

  const refreshFocusedThreads = () => {
    if (focusedSessionId !== null) {
      invalidateSessionThreads(queryClient, focusedSessionId);
    }
  };

  switch (event.kind) {
    case 'send_dispatched':
      // A held (`queued`) send was promoted to `dispatched` and typed. Only
      // the send queue moved (no transcript change yet), so refetch the
      // session's open-send list to show the chip's queued→dispatched flip.
      invalidateSessionSends(queryClient, event.session_id);
      break;
    case 'turn_started':
    case 'turn_completed':
    case 'turn_interrupted':
      // The send queue moved (a send matched, or the turn that drains it
      // ended); refetch the session's open-send list regardless of focus so a
      // background session's pending strip is correct the moment it is viewed.
      invalidateSessionSends(queryClient, event.session_id);
      // A turn finishing on a thread the user is not currently viewing produced
      // something unseen, so bump THAT thread's unread — the navigator shows it
      // on the thread (and OR-aggregates it onto the collapsed session row).
      // Only on `turn_completed` (the Stop hook — the genuine "it finished"
      // signal); an interrupt is the user's own Escape/Ctrl-C, not a surprise
      // that needs flagging. "Not viewing" means a background session, or the
      // focused session but a thread other than the active one — so finishing
      // the very thread on screen never raises a badge.
      if (
        event.kind === 'turn_completed' &&
        event.thread_id !== null &&
        !(isFocused && activeThreadId === event.thread_id)
      ) {
        store.bumpUnread(event.thread_id);
      }
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
      // affected thread plus the focused active one, with no store change.
      //
      // New lines also move the session's last activity — and with it the
      // session list's most-recently-active ordering — so refresh the list
      // too. And an ingested user line is what matches a dispatched send
      // (terminal — it leaves the open list), so the session's open-send list
      // refetches here as well.
      invalidateSessions(queryClient);
      invalidateSessionSends(queryClient, event.session_id);
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
      if (event.kind === 'session_closed') {
        // Closing cancels/strands nothing silently: refetch so the pending
        // strip reflects whatever the server did with the queue.
        invalidateSessionSends(queryClient, event.session_id);
      }
      if (isFocused) {
        refreshFocusedThreads();
      }
      break;
    case 'permission_requested':
    case 'permission_resolved':
    case 'question_asked':
      // Pure UI notice (set/cleared); already handled by the store. A
      // `question_asked` (AskUserQuestion) clears via the same
      // `permission_resolved` the correlated tool_result emits.
      break;
    case 'assistant_streaming':
      // The live preview is held in the store (appended above by
      // `store.applyEvent`) and read straight from there by the transcript
      // pane, so there is no query cache to invalidate. The persisted message
      // arrives later via the normal transcript sync (a `transcript_updated` /
      // turn-end refetch), which is what supersedes the preview.
      break;
    case 'spawn_failed':
      // A freshly-spawned session never bound; the server reaped its row (the
      // store flips the tracked spawn to a Retry/Dismiss chip). Drop the
      // session's cached open sends — the row is gone, so a refetch would only
      // 404. No session-list refetch: a message-less spawning session was
      // never listed, so the list cannot lose a row for it.
      removeSessionSends(queryClient, event.session_id);
      break;
  }
}
