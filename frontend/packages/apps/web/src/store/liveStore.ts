import { create } from 'zustand';
import type {
  MessageUuid,
  SessionEvent,
  SessionId,
  ThreadId,
} from '@delta/model';
import type { ConnectionStatus } from '@delta/api-client';

/**
 * Ephemeral, session-only live UI state that is NOT a REST resource. REST
 * resources (session, threads, messages) live in the TanStack Query cache;
 * this store holds only volatile UI signals derived from the live channel and
 * from optimistic local actions.
 */

/** A locally-tracked pending send, shown optimistically in the composer pane. */
export interface PendingItem {
  /** Client-side id assigned before the server responds. */
  localId: string;
  /** Server id once `POST /api/sends` resolves. */
  sendId: number | null;
  /**
   * The session this send belongs to, or `null` for a new-session send whose
   * id is not bound yet. Turn events are scoped to their `session_id` so a turn
   * completing in one session never drains another session's queued send.
   */
  sessionId: SessionId | null;
  threadId: ThreadId;
  text: string;
  semanticParentUuid: MessageUuid | null;
  /** queued: waiting in the FIFO; in_progress: turn started; done/failed terminal. */
  status: 'queued' | 'in_progress' | 'done' | 'failed';
  createdAt: number;
}

export interface PermissionNotice {
  requestId: number;
  toolName: string;
}

export interface ExternalInputMarker {
  threadId: ThreadId;
  prompt: string;
  at: number;
}

export interface LiveState {
  connection: ConnectionStatus;
  /** FIFO of optimistic sends, oldest first. */
  pending: PendingItem[];
  /**
   * Permission requests keyed by the session blocked on them. A tool's
   * PreToolUse hook blocks that session until the prompt is answered in its
   * terminal, so the notice is per-session: the focused session's drives the
   * inline notice above the composer, and any session's drives a badge on its
   * navigator row. Cleared on dismiss, when the session's turn completes, and
   * when the session closes.
   */
  permission: Record<SessionId, PermissionNotice>;
  /** Unread counts keyed by thread id; cleared when a thread becomes active. */
  unread: Record<ThreadId, number>;
  /** The most recent external (direct-pane) input, shown on the last active thread. */
  externalInput: ExternalInputMarker | null;
  /**
   * Sessions a Send/open just failed to resume because their transcript is gone
   * (the server's `resume_unavailable`). The focused session's presence here
   * drives an inline "cannot be resumed" notice; the session stays closed and
   * no optimistic pending chip is shown. Cleared when the session opens.
   */
  resumeUnavailable: Record<SessionId, true>;

  setConnection: (status: ConnectionStatus) => void;
  enqueueSend: (item: PendingItem) => void;
  attachSendId: (localId: string, sendId: number) => void;
  /**
   * Re-key a pending send to a different thread. A branch send is enqueued under
   * its parent thread (the child does not exist yet); once the server creates
   * the child and the view drills into it, the pending entry is moved to the
   * child so the "waiting" indicator follows the user into the sub-thread.
   */
  retargetSend: (localId: string, threadId: ThreadId) => void;
  failSend: (localId: string) => void;
  /**
   * Drop an optimistic pending send outright (not merely mark it failed). Used
   * when a Send is rejected before it could ever queue server-side — e.g. a
   * resume-unavailable session — so no "waiting" chip lingers for a turn that
   * will never start.
   */
  removePending: (localId: string) => void;
  /** Flag a session as resume-impossible, surfacing the inline notice. */
  markResumeUnavailable: (sessionId: SessionId) => void;
  /** Clear a session's resume-impossible flag (e.g. once it opens). */
  clearResumeUnavailable: (sessionId: SessionId) => void;
  /**
   * Drop every optimistic pending send. Used on a live-stream reconnect: the
   * `turn_completed` events that would have drained these were broadcast while
   * the socket was down and are not replayed, so the FIFO can no longer be
   * reconciled from events. The refetched transcript is the source of truth for
   * what actually landed, so clearing the stale optimistic chips is correct.
   */
  clearPending: () => void;
  bumpUnread: (threadId: ThreadId) => void;
  clearUnread: (threadId: ThreadId) => void;
  /** Record an external (direct-pane) input marker on a thread. */
  noteExternalInput: (threadId: ThreadId, prompt: string) => void;
  /** Dismiss the permission notice for a session. */
  dismissPermission: (sessionId: SessionId) => void;
  /**
   * Apply a live session event, mutating only session-scoped ephemeral state
   * (the pending FIFO, permission notice). Focus-dependent signals (the
   * external-input marker, unread badges) are recorded by the router under a
   * focus guard, not here.
   */
  applyEvent: (event: SessionEvent) => void;
}

/**
 * Find the FIFO index of the oldest pending item that (a) belongs to
 * `sessionId` and (b) satisfies `predicate`. Turn events carry a `session_id`,
 * so matching is scoped to that session to keep one session's turn from
 * draining another's queue. An unbound new-session item (`sessionId === null`)
 * has no id to compare yet, so it is accepted only as a fallback when no
 * exact-session item matches — that is the event that finally clears it once
 * its session has bound.
 */
function matchPendingIndex(
  pending: PendingItem[],
  sessionId: SessionId,
  predicate: (item: PendingItem) => boolean,
): number {
  const exact = pending.findIndex(
    (item) => item.sessionId === sessionId && predicate(item),
  );
  if (exact !== -1) {
    return exact;
  }
  return pending.findIndex(
    (item) => item.sessionId === null && predicate(item),
  );
}

export const useLiveStore = create<LiveState>((set) => ({
  connection: 'connecting',
  pending: [],
  permission: {},
  unread: {},
  externalInput: null,
  resumeUnavailable: {},

  setConnection: (status) => set({ connection: status }),

  enqueueSend: (item) =>
    set((state) => ({ pending: [...state.pending, item] })),

  attachSendId: (localId, sendId) =>
    set((state) => ({
      pending: state.pending.map((item) =>
        item.localId === localId ? { ...item, sendId } : item,
      ),
    })),

  retargetSend: (localId, threadId) =>
    set((state) => ({
      pending: state.pending.map((item) =>
        item.localId === localId ? { ...item, threadId } : item,
      ),
    })),

  failSend: (localId) =>
    set((state) => ({
      pending: state.pending.map((item) =>
        item.localId === localId ? { ...item, status: 'failed' } : item,
      ),
    })),

  removePending: (localId) =>
    set((state) => ({
      pending: state.pending.filter((item) => item.localId !== localId),
    })),

  markResumeUnavailable: (sessionId) =>
    set((state) =>
      state.resumeUnavailable[sessionId]
        ? state
        : {
            resumeUnavailable: { ...state.resumeUnavailable, [sessionId]: true },
          },
    ),

  clearResumeUnavailable: (sessionId) =>
    set((state) => {
      if (!state.resumeUnavailable[sessionId]) {
        return state;
      }
      const next = { ...state.resumeUnavailable };
      delete next[sessionId];
      return { resumeUnavailable: next };
    }),

  clearPending: () => set({ pending: [] }),

  bumpUnread: (threadId) =>
    set((state) => ({
      unread: { ...state.unread, [threadId]: (state.unread[threadId] ?? 0) + 1 },
    })),

  clearUnread: (threadId) =>
    set((state) => {
      if (!state.unread[threadId]) {
        return state;
      }
      const next = { ...state.unread };
      delete next[threadId];
      return { unread: next };
    }),

  noteExternalInput: (threadId, prompt) =>
    set({ externalInput: { threadId, prompt, at: Date.now() } }),

  dismissPermission: (sessionId) =>
    set((state) => {
      if (!state.permission[sessionId]) {
        return state;
      }
      const permission = { ...state.permission };
      delete permission[sessionId];
      return { permission };
    }),

  applyEvent: (event) =>
    set((state) => {
      switch (event.kind) {
        case 'turn_started': {
          // Promote the head queued send for THIS session to in-progress (FIFO
          // order). Scoping by session keeps a turn in one session from touching
          // another session's queue. An unbound new-session item (sessionId
          // null) matches only when no exact-session item does.
          const idx = matchPendingIndex(
            state.pending,
            event.session_id,
            (item) =>
              item.status === 'queued' &&
              (item.sendId === event.pending_send_id || item.sendId === null),
          );
          if (idx === -1) {
            return state;
          }
          const pending = state.pending.slice();
          pending[idx] = { ...pending[idx], status: 'in_progress' };
          return { pending };
        }
        case 'turn_completed': {
          // The turn ended, so any permission prompt that was blocking THIS
          // session is resolved — clear it. Also drop the oldest active send for
          // this session from the visible FIFO. It may still be `queued` rather
          // than `in_progress`: `turn_started` only fires when the user line was
          // ingested in the same `UserPromptSubmit` sync, which often does not
          // happen, so a completed turn must clear a still-queued send too —
          // otherwise it stays "waiting" forever. Scoped by session so a turn in
          // one session never drains another session's queue.
          const next: Partial<LiveState> = {};
          const idx = matchPendingIndex(
            state.pending,
            event.session_id,
            (item) =>
              item.status === 'in_progress' || item.status === 'queued',
          );
          if (idx !== -1) {
            next.pending = state.pending.filter((_, i) => i !== idx);
          }
          if (state.permission[event.session_id]) {
            const permission = { ...state.permission };
            delete permission[event.session_id];
            next.permission = permission;
          }
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'permission_requested':
          return {
            permission: {
              ...state.permission,
              [event.session_id]: {
                requestId: event.request_id,
                toolName: event.tool_name,
              },
            },
          };
        case 'external_input':
          // The external-input marker is session-scoped and only meaningful for
          // the focused session, so the router (`applySessionEvent`) records it
          // via `noteExternalInput` under a focus guard. Nothing to do here.
          return state;
        case 'session_registered':
          // Open/closed lifecycle is reflected by the sessions query, not
          // ephemeral here.
          return state;
        case 'session_opened': {
          // The session resumed successfully, so any stale "cannot be resumed"
          // notice for it is now wrong — clear it. Open/closed itself is
          // reflected by the sessions query, not ephemeral here.
          if (!state.resumeUnavailable[event.session_id]) {
            return state;
          }
          const resumeUnavailable = { ...state.resumeUnavailable };
          delete resumeUnavailable[event.session_id];
          return { resumeUnavailable };
        }
        case 'session_closed': {
          // Closed state itself is reflected by the sessions query. But a closed
          // session has no live process, so any permission prompt for it is moot
          // — clear it.
          if (!state.permission[event.session_id]) {
            return state;
          }
          const permission = { ...state.permission };
          delete permission[event.session_id];
          return { permission };
        }
        default:
          return state;
      }
    }),
}));
