import type { QueryClient } from '@tanstack/react-query';
import type { SessionId, ThreadId } from '@delta/model';
import type { SessionEvent } from '@delta/wire-gen';
import {
  invalidateRepositoriesAndPullRequests,
  invalidateSessions,
  invalidateSessionSends,
  invalidateSessionThreads,
  invalidateThreadMessages,
  removeSessionSends,
} from '@delta/api-client';
import { useLiveStore } from '../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../store/navStore';

/**
 * Route a live `SessionEvent` to the two state homes:
 *
 * - **Query cache** (`@tanstack/react-query`): incremental REST-resource growth.
 *   The `/ws` events do not carry message bodies, so we patch the cache by
 *   invalidating the affected `messages`/`session-threads` queries, which
 *   refetches the freshly-ingested transcript lines. Send-affecting events
 *   (the turn lifecycle, transcript growth, a close) also invalidate the
 *   session's open-send list — the server-side truth behind the pending strip.
 *   Lifecycle events (`session_registered`/`session_opened`/`session_closed`,
 *   and `spawn_failed` — a reaped spawn's row is deleted) invalidate the
 *   session list so a starting, registered, resumed, closed, or vanished
 *   session's presence and open flag stay in sync.
 * - **Nav store** (Zustand): one case only — a `spawn_failed` for the focused
 *   session, which is about to stop existing, so focus is handed back to the
 *   new-session screen where its Retry / Dismiss card lives (and where the live
 *   store has just restored whatever the failed launch never sent).
 * - **Live store** (Zustand): ephemeral UI signals that are not REST resources
 *   — turn tracking, the spawn registry, permission notices, unread badges,
 *   external input, and the per-session resuming marker.
 *
 * Transcript/turn events are scoped to the focused session: `activeThreadId`
 * selects which transcript to refetch and which thread to badge, and
 * `focusedSessionId` selects whose thread tree to refresh. Events for a
 * non-focused session still refresh the session list and that session's
 * open-send list, but never touch the focused transcript.
 *
 * The repository-clone events are the one family naming no session at all: they
 * report a workspace-level job, so they are routed by repository rather than by
 * focus — refreshing the repository and PR lists, and leaving it to the live
 * store to decide (from the active clone intent) whether this browser is the one
 * that was waiting for it.
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

  // Almost every event names the session it concerns; the repository-clone pair
  // does not (cloning is a workspace-level job with no session behind it), so
  // the field is read through a presence check rather than off the bare union.
  const eventSessionId = 'session_id' in event ? event.session_id : null;
  const isFocused =
    focusedSessionId !== null && eventSessionId === focusedSessionId;

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
      // Refetch the thread the event names — unconditionally, not gated on
      // focus or `activeThreadId`. Under slow scheduling a `turn_started` WS
      // frame for a freshly-spawned session can arrive BEFORE `setFocusedSession`
      // + `setActiveThread` settle: with the previous gate the invalidate was
      // skipped, and the next refetch trigger became the next turn event seconds
      // later — by which point the user prompt, streamed reply, and tool_use
      // lines had all landed together on the same refetch, surfacing as 3
      // message-items in the streaming-window of `streaming.spec.ts` where the
      // assertion expects 1. Routing by `event.thread_id` (always carried by
      // `turn_started`; carried by turn completion/interruption when the turn
      // was thread-bound) targets exactly the thread the server says grew,
      // independent of client focus state — and invalidate on a not-yet-mounted
      // observer is a safe no-op, so a refetch only fires where one is needed.
      // An interrupt also appends the `[Request interrupted by user]` marker
      // line, so it refetches the same way a completed turn does.
      if (event.thread_id !== null) {
        invalidateThreadMessages(queryClient, event.thread_id);
      }
      // A session-wide turn end (`thread_id: null`, e.g. a turn that has no
      // bound thread) still needs the focused active thread to refetch — its
      // transcript may have grown via that session-level signal even though no
      // specific thread was named. Skip the duplicate when the event already
      // named the active thread above.
      if (
        isFocused &&
        activeThreadId !== null &&
        activeThreadId !== event.thread_id
      ) {
        invalidateThreadMessages(queryClient, activeThreadId);
      }
      // A branch send may have created a new thread; keep the tree fresh.
      if (isFocused) {
        refreshFocusedThreads();
      }
      break;
    case 'send_parked':
      // The server gave up delivering a composed message and put its row back
      // in the queue, held for an explicit release, so the open-send list
      // changed: refetch it (regardless of focus) so the chip that would
      // otherwise spin forever becomes the held row with its Send and Cancel
      // controls. The "why" is already in the store as a notice.
      invalidateSessionSends(queryClient, event.session_id);
      break;
    case 'external_input':
      // Direct-pane input lands on the focused session's active thread. The
      // marker is recorded only for the focused session so a background
      // session's typing never surfaces on the transcript the user is viewing.
      //
      // Deliberately NO unread bump, unlike `turn_completed` above. The wire
      // event carries no `thread_id`, so the only thread it can be attributed
      // to is the focused ACTIVE one — the thread on screen, read by
      // definition. That is the invariant the `turn_completed` guard enforces.
      //
      // The bump that used to be here was also unclearable: a badge is
      // suppressed while its thread is active (ThreadTree), and back when
      // `clearUnread` fired only on the activation edge (see WorkspaceScreen),
      // an already active thread never crossed that edge again. The count sat
      // unseen until the user switched away, then surfaced as a phantom. The
      // deactivation edge now clears such counts as a backstop, but the right
      // fix is not to create one: the user-visible record of the input is the
      // dismissible notice that `noteExternalInput` records below, already on
      // screen when the input arrives.
      if (isFocused && focusedSessionId !== null && activeThreadId !== null) {
        invalidateThreadMessages(queryClient, activeThreadId);
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
    case 'subagent_started':
    case 'subagent_finished':
      // The store recorded it above, but this also moved the session's
      // QUERYABLE live state: the open-sends envelope carries
      // `running_subagents`, and `usePendingSends` re-seeds the store from it
      // authoritatively — an empty list there clears whatever the store holds,
      // which is exactly what lets that envelope heal a reconnect.
      //
      // So an envelope the server computed BEFORE this event, still in flight
      // when the event lands, would wipe the subagent it just recorded and
      // leave the pane looking idle for the rest of the turn. Refetching here
      // closes that race from both ends: the in-flight request is superseded
      // (its stale result dropped rather than applied), and the response that
      // does land was computed after the server recorded the change.
      invalidateSessionSends(queryClient, event.session_id);
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
      // store flips the tracked spawn to a Retry/Dismiss chip and restores
      // `event.unsent` into the new-session composer draft). That restore
      // happens in `store.applyEvent` above, i.e. strictly before the focus
      // handoff below, so the composer mounts with the text already in place.
      // Drop the session's cached open sends — the
      // row is gone, so a refetch would only 404 — and refetch the session
      // list, which was listing the starting session and must now lose it.
      removeSessionSends(queryClient, event.session_id);
      invalidateSessions(queryClient);
      // The user is very likely looking at it: the workspace focuses a
      // starting session the moment its first send is accepted. Its screen is
      // about to describe a session that no longer exists, and the failure's
      // Retry / Dismiss card renders on the new-session surface (see
      // `usePendingSends`) — so send focus back there, where the user can act
      // on it. Reconciling rather than navigating leaves any overlay they
      // opened in the meantime standing.
      if (isFocused) {
        useNavStore.getState().reconcileFocusedSession(NEW_SESSION_FOCUS);
      }
      break;
    case 'repository_clone_completed':
    case 'repository_clone_failed':
      // A clone job reported. Refetch the repository list and both PR lenses
      // unconditionally — even for a failure, and even when this browser never
      // asked for the clone: `has_local_clone` is a fact about the filesystem,
      // and another tab (or another window) may have been the one to change it.
      // Whether this browser *acts* on the completion is the clone slice's
      // decision, made from the active intent, not this refetch's.
      invalidateRepositoriesAndPullRequests(queryClient);
      break;
  }
}
