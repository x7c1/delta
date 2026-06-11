import { create } from 'zustand';
import type { MessageUuid, SessionId, ThreadId } from '@delta/model';
import type { SessionEvent } from '@delta/wire-gen';
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
  /**
   * The working directory chosen for a new-session send, retained so a failed
   * spawn can be retried with the same directory. `null`/absent for the default
   * directory and for non-new-session sends (which have no directory choice).
   */
  workdir?: string | null;
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
  /**
   * External (direct-pane) input markers keyed by the session they landed on.
   * Someone typing straight into a session's embedded terminal (rather than
   * sending through the composer) surfaces an inline notice above the composer.
   * Like {@link permission}, the marker is per-session and cleared on dismiss,
   * when the session's turn completes, and when the session closes — otherwise
   * the notice would linger forever once shown. The retained `threadId` lets the
   * transcript pane gate visibility to the focused thread.
   */
  externalInput: Record<SessionId, ExternalInputMarker>;
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
  /**
   * Bind the unbound new-session pending send to the session and main thread it
   * spawned, once that session registers and its main thread id becomes known. A
   * new-session send is enqueued under the new-session sentinel thread with no
   * session id (the spawn has no real ids yet), so the optimistic strip — keyed
   * by the active thread — would otherwise vanish the instant the view navigates
   * to the freshly-spawned session, even while its first turn is still running.
   * Re-keys the oldest unbound send (`sessionId === null`); a no-op when none is
   * unbound (the send already drained, or there was no new-session send).
   */
  bindNewSessionPending: (sessionId: SessionId, threadId: ThreadId) => void;
  failSend: (localId: string) => void;
  /**
   * Mark a failed-spawn new-session pending as `failed`, surfacing the recoverable
   * error chip. A spawn that never came up never registered, so its optimistic
   * send is still an unbound new-session pending (`sessionId === null`). The
   * `spawn_failed` event carries the minted `session_id`/`pane_token`, but the
   * unbound pending never learned either id (binding only happens on a successful
   * registration), so there is nothing to correlate against — we mark the OLDEST
   * unbound new-session pending. With at most one new-session spawn in flight at a
   * time this is exact; were several queued at once it could mismatch which one,
   * but that is still strictly better than the silent stuck-pending status quo.
   * Already-`failed` pendings are skipped so a second failure marks a different
   * one. No-op when no unbound new-session pending remains.
   */
  failSpawn: () => void;
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
  /** Record an external (direct-pane) input marker for a session/thread. */
  noteExternalInput: (
    sessionId: SessionId,
    threadId: ThreadId,
    prompt: string,
  ) => void;
  /** Dismiss the permission notice for a session. */
  dismissPermission: (sessionId: SessionId) => void;
  /** Dismiss the external-input notice for a session. */
  dismissExternalInput: (sessionId: SessionId) => void;
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

/**
 * Compute the state changes for a turn ending in `sessionId`: drop the oldest
 * active (in_progress or queued) pending send for that session and clear any
 * session-scoped permission / external-input notices. Returns only the changed
 * slices (empty object when nothing matched, so the caller can keep the
 * identity-stable `state`). Shared by `turn_completed` (the `Stop` hook) and
 * `turn_interrupted` (the transcript-detected interrupt), which must drain the
 * stuck pending the same way; on interrupt the `Stop` hook never fires, and the
 * two can occasionally both arrive, so the drain is idempotent (a no-match is a
 * no-op).
 */
function drainTurnForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  const next: Partial<LiveState> = {};
  const idx = matchPendingIndex(
    state.pending,
    sessionId,
    (item) => item.status === 'in_progress' || item.status === 'queued',
  );
  if (idx !== -1) {
    next.pending = state.pending.filter((_, i) => i !== idx);
  }
  if (state.permission[sessionId]) {
    const permission = { ...state.permission };
    delete permission[sessionId];
    next.permission = permission;
  }
  if (state.externalInput[sessionId]) {
    const externalInput = { ...state.externalInput };
    delete externalInput[sessionId];
    next.externalInput = externalInput;
  }
  return next;
}

export const useLiveStore = create<LiveState>((set) => ({
  connection: 'connecting',
  pending: [],
  permission: {},
  unread: {},
  externalInput: {},
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

  bindNewSessionPending: (sessionId, threadId) =>
    set((state) => {
      // Bind the oldest unbound new-session pending — but never a `failed` one:
      // a failed spawn is terminal and waiting on the user to retry or dismiss,
      // so a later successful spawn must bind to a live (queued/in_progress)
      // pending, not resurrect the failed chip onto a real session.
      const idx = state.pending.findIndex(
        (item) => item.sessionId === null && item.status !== 'failed',
      );
      if (idx === -1) {
        return state;
      }
      const pending = state.pending.slice();
      pending[idx] = { ...pending[idx], sessionId, threadId };
      return { pending };
    }),

  failSend: (localId) =>
    set((state) => ({
      pending: state.pending.map((item) =>
        item.localId === localId ? { ...item, status: 'failed' } : item,
      ),
    })),

  failSpawn: () =>
    set((state) => {
      const idx = state.pending.findIndex(
        (item) => item.sessionId === null && item.status !== 'failed',
      );
      if (idx === -1) {
        return state;
      }
      const pending = state.pending.slice();
      pending[idx] = { ...pending[idx], status: 'failed' };
      return { pending };
    }),

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

  noteExternalInput: (sessionId, threadId, prompt) =>
    set((state) => ({
      externalInput: {
        ...state.externalInput,
        [sessionId]: { threadId, prompt, at: Date.now() },
      },
    })),

  dismissPermission: (sessionId) =>
    set((state) => {
      if (!state.permission[sessionId]) {
        return state;
      }
      const permission = { ...state.permission };
      delete permission[sessionId];
      return { permission };
    }),

  dismissExternalInput: (sessionId) =>
    set((state) => {
      if (!state.externalInput[sessionId]) {
        return state;
      }
      const externalInput = { ...state.externalInput };
      delete externalInput[sessionId];
      return { externalInput };
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
          // session is resolved, any external-input notice has served its
          // purpose, and the oldest active send for this session is drained from
          // the visible FIFO. The send may still be `queued` rather than
          // `in_progress`: `turn_started` only fires when the user line was
          // ingested in the same `UserPromptSubmit` sync, which often does not
          // happen, so a completed turn must clear a still-queued send too —
          // otherwise it stays "waiting" forever. Scoped by session so a turn in
          // one session never drains another session's queue.
          const next = drainTurnForSession(state, event.session_id);
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'turn_interrupted': {
          // The user interrupted the in-flight turn (Escape / Ctrl-C). Claude's
          // `Stop` hook does not fire on interrupt, so `turn_completed` never
          // arrives and the optimistic pending chip would stay "in progress"
          // forever. The backend detects the interrupt from the transcript and
          // emits this hook-independent signal; drain the stuck send exactly as
          // a completed turn would. Idempotent — `Stop` may occasionally also
          // fire, and a no-match is a no-op.
          const next = drainTurnForSession(state, event.session_id);
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
        case 'permission_resolved': {
          // The correlated tool_result was ingested, so the request is done.
          // Clear the notice only when it is the SAME request that resolved, so
          // a stale resolution never wipes a newer pending prompt for the same
          // session. An auto-approved tool resolves almost immediately, so this
          // clears the brief notice (hidden by the render debounce); a genuine
          // prompt has no resolution until the human answers.
          const current = state.permission[event.session_id];
          if (!current || current.requestId !== event.request_id) {
            return state;
          }
          const permission = { ...state.permission };
          delete permission[event.session_id];
          return { permission };
        }
        case 'spawn_failed':
          // The failed-spawn chip is driven by the dedicated `failSpawn` action,
          // routed from `applySessionEvent` (it has no session-scoped FIFO drain
          // semantics to share with the turn cases here). Nothing to do here.
          return state;
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
          // session has no live process, so any permission prompt or stale
          // external-input notice for it is moot — clear both.
          const next: Partial<LiveState> = {};
          if (state.permission[event.session_id]) {
            const permission = { ...state.permission };
            delete permission[event.session_id];
            next.permission = permission;
          }
          if (state.externalInput[event.session_id]) {
            const externalInput = { ...state.externalInput };
            delete externalInput[event.session_id];
            next.externalInput = externalInput;
          }
          return Object.keys(next).length > 0 ? next : state;
        }
        default:
          return state;
      }
    }),
}));
