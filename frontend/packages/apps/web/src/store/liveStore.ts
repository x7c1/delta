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
  permission: PermissionNotice | null;
  /** Unread counts keyed by thread id; cleared when a thread becomes active. */
  unread: Record<ThreadId, number>;
  /** The most recent external (direct-pane) input, shown on the last active thread. */
  externalInput: ExternalInputMarker | null;

  setConnection: (status: ConnectionStatus) => void;
  enqueueSend: (item: PendingItem) => void;
  attachSendId: (localId: string, sendId: number) => void;
  failSend: (localId: string) => void;
  bumpUnread: (threadId: ThreadId) => void;
  clearUnread: (threadId: ThreadId) => void;
  /** Record an external (direct-pane) input marker on a thread. */
  noteExternalInput: (threadId: ThreadId, prompt: string) => void;
  dismissPermission: () => void;
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
  permission: null,
  unread: {},
  externalInput: null,

  setConnection: (status) => set({ connection: status }),

  enqueueSend: (item) =>
    set((state) => ({ pending: [...state.pending, item] })),

  attachSendId: (localId, sendId) =>
    set((state) => ({
      pending: state.pending.map((item) =>
        item.localId === localId ? { ...item, sendId } : item,
      ),
    })),

  failSend: (localId) =>
    set((state) => ({
      pending: state.pending.map((item) =>
        item.localId === localId ? { ...item, status: 'failed' } : item,
      ),
    })),

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

  dismissPermission: () => set({ permission: null }),

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
          // Drop the oldest active send for THIS session from the visible FIFO.
          // It may still be `queued` rather than `in_progress`: `turn_started`
          // only fires when the user line was ingested in the same
          // `UserPromptSubmit` sync, which often does not happen, so a completed
          // turn must clear a still-queued send too — otherwise it stays
          // "waiting" forever. Scoped by session so a turn in one session never
          // drains another session's queue.
          const idx = matchPendingIndex(
            state.pending,
            event.session_id,
            (item) =>
              item.status === 'in_progress' || item.status === 'queued',
          );
          if (idx === -1) {
            return state;
          }
          const pending = state.pending.filter((_, i) => i !== idx);
          return { pending };
        }
        case 'permission_requested':
          return {
            permission: {
              requestId: event.request_id,
              toolName: event.tool_name,
            },
          };
        case 'external_input':
          // The external-input marker is session-scoped and only meaningful for
          // the focused session, so the router (`applySessionEvent`) records it
          // via `noteExternalInput` under a focus guard. Nothing to do here.
          return state;
        case 'session_registered':
        case 'session_opened':
          // Open/closed lifecycle is reflected by the sessions query, not
          // ephemeral here.
          return state;
        case 'session_closed':
          // Closed state is reflected by the sessions query, not ephemeral here.
          return state;
        default:
          return state;
      }
    }),
}));
