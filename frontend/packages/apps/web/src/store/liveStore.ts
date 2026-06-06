import { create } from 'zustand';
import type { MessageUuid, SessionEvent, ThreadId } from '@delta/model';
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
  dismissPermission: () => void;
  /** Apply a live session event, mutating only ephemeral state. */
  applyEvent: (event: SessionEvent, activeThreadId: ThreadId | null) => void;
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

  dismissPermission: () => set({ permission: null }),

  applyEvent: (event, activeThreadId) =>
    set((state) => {
      switch (event.kind) {
        case 'turn_started': {
          // Promote the head queued send to in-progress (FIFO order).
          const idx = state.pending.findIndex(
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
          // Drop the oldest active send from the visible FIFO. It may still be
          // `queued` rather than `in_progress`: `turn_started` only fires when
          // the user line was ingested in the same `UserPromptSubmit` sync,
          // which often does not happen, so a completed turn must clear a still-
          // queued send too — otherwise it stays "waiting" forever.
          const idx = state.pending.findIndex(
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
          return {
            externalInput: {
              threadId: activeThreadId ?? 0,
              prompt: event.prompt,
              at: Date.now(),
            },
          };
        case 'session_registered':
          return state;
        default:
          return state;
      }
    }),
}));
