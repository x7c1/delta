import { create } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { SessionEvent, Turn } from '@delta/wire-gen';
import type { ConnectionStatus } from '@delta/api-client';

/**
 * Ephemeral, session-only live UI state that is NOT a REST resource. REST
 * resources (session, threads, messages, the open-send list) live in the
 * TanStack Query cache; this store holds only volatile UI signals derived from
 * the live channel and from optimistic local actions.
 *
 * The pending-send strip is server-authoritative: its rows come from
 * `GET /api/sessions/{id}/sends` via the query cache. This store keeps only
 * the three client-side complements the server cannot provide yet:
 *
 * - {@link LiveState.sending} — a submit whose `POST /api/sends` has not
 *   resolved (the server knows nothing about it yet), or whose POST failed.
 * - {@link LiveState.localSends} — an accepted send tracked until its turn
 *   ends. A send leaves the server's open list the moment it correlates with
 *   its transcript line (status `matched`), which is usually *while* its turn
 *   is still running; this local twin keeps the chip visible until the
 *   `turn_completed`/`turn_interrupted` event actually lands.
 * - {@link LiveState.spawns} — a new-session spawn tracked from the POST
 *   response until it registers or fails. The server deletes a failed spawn's
 *   contentless row at reap, so the failure chip (and its Retry payload)
 *   cannot be server-rendered.
 */

/**
 * A submit the server has not accepted yet (its `POST /api/sends` is in
 * flight), or that the server rejected (`status: 'failed'`). Keyed by the
 * surface the composer rendered under, so the chip shows exactly where the
 * user pressed Send.
 */
export interface SendingItem {
  /** Client-side id (the server has not issued one yet). */
  id: string;
  /**
   * Where the submit happened: an existing thread, or the new-session
   * composer (which retains the chosen `workdir` so a failed launch request
   * can be retried with the same directory).
   */
  target:
    | { kind: 'thread'; sessionId: SessionId; threadId: ThreadId }
    | { kind: 'new-session'; workdir: string | null };
  text: string;
  /** sending: POST in flight; failed: POST rejected (dismiss or retry). */
  status: 'sending' | 'failed';
  createdAt: number;
}

/**
 * A server-accepted send, tracked locally until its turn ends. Keyed by the
 * REAL ids from the `POST /api/sends` response — never a sentinel.
 */
export interface LocalSend {
  sendId: number;
  sessionId: SessionId;
  threadId: ThreadId;
  text: string;
  createdAt: number;
}

/** A new-session spawn tracked from the POST response (real ids). */
export interface SpawnItem {
  sessionId: SessionId;
  /** The spawned session's `main` thread (from the POST response). */
  threadId: ThreadId;
  /** The first prompt, retained so a failed spawn can be retried. */
  text: string;
  /** The chosen working directory, retained for the same Retry. */
  workdir: string | null;
  /** spawning: launch in flight; failed: reaped (`spawn_failed` arrived). */
  status: 'spawning' | 'failed';
}

export interface PermissionNotice {
  requestId: number;
  toolName: string;
  /** The tool input, serialized as JSON text (shown summarized). */
  toolInput: string;
}

export interface ExternalInputMarker {
  threadId: ThreadId;
  prompt: string;
  at: number;
}

export interface LiveState {
  connection: ConnectionStatus;
  /** Submits not yet accepted (or rejected) by the server, oldest first. */
  sending: SendingItem[];
  /**
   * Accepted sends whose turn has not ended, keyed by send id. Rendered as an
   * in-progress chip only while the send is absent from the server's open
   * list (i.e. it already matched); drained by the turn-end events.
   */
  localSends: Record<number, LocalSend>;
  /** Tracked new-session spawns, oldest first, keyed by real session id. */
  spawns: SpawnItem[];
  /**
   * Sessions with a turn in flight, set by `turn_started` and cleared when
   * the turn completes or is interrupted (or the session closes). Drives the
   * navigator's "running" indicator. Note `turn_started` only fires when the
   * user line was ingested in the same `UserPromptSubmit` sync, so absence
   * here does not prove idleness — same semantics the FIFO's `in_progress`
   * status had.
   */
  activeTurns: Record<SessionId, true>;
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
  /** Record a submit whose `POST /api/sends` is about to fly. */
  beginSending: (item: SendingItem) => void;
  /** Mark an in-flight submit rejected, surfacing the recoverable chip. */
  failSending: (id: string) => void;
  /** Drop a submit chip (POST accepted, or the failure dismissed). */
  removeSending: (id: string) => void;
  /** Track an accepted send until its turn ends (real ids from the POST). */
  recordLocalSend: (send: LocalSend) => void;
  /** Track a new-session spawn (real ids from the POST response). */
  trackSpawn: (spawn: Omit<SpawnItem, 'status'>) => void;
  /** Drop a tracked spawn (it registered, or its failure was dismissed). */
  clearSpawn: (sessionId: SessionId) => void;
  /** Flag a session as resume-impossible, surfacing the inline notice. */
  markResumeUnavailable: (sessionId: SessionId) => void;
  /** Clear a session's resume-impossible flag (e.g. once it opens). */
  clearResumeUnavailable: (sessionId: SessionId) => void;
  /**
   * Drop the event-reconstructed turn state (tracked local sends and the
   * active-turn flags). Used on a live-stream reconnect: the turn-end events
   * that would have drained these were broadcast while the socket was down and
   * are not replayed, so they can no longer be reconciled from events. The
   * server's open-send list recovers by refetch, and {@link seedActiveTurn}
   * re-seeds the active-turn flag from that refetch's `turn` field.
   */
  resetTurnEphemera: () => void;
  /**
   * Seed a session's active-turn flag from the server's queryable turn state
   * (the `turn` field of `GET /api/sessions/{id}/sends`). Set-only, and only
   * for `in_flight` — the phase `turn_started` would have announced (healing
   * a reconnect that missed it). `awaiting_echo` is a dispatch whose turn has
   * not started yet, exactly like a live `send_dispatched`, and `idle`
   * changes nothing — clearing is owned by the turn-end events, so a
   * momentarily-stale refetch can never wipe a flag an event just set.
   */
  seedActiveTurn: (sessionId: SessionId, turn: Turn) => void;
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
   * (turn tracking, the spawn registry, the permission notice). Focus-dependent
   * signals (the external-input marker, unread badges) are recorded by the
   * router under a focus guard, not here.
   */
  applyEvent: (event: SessionEvent) => void;
}

/**
 * Compute the state changes for a turn ending in `sessionId`: drop the tracked
 * local sends for that session (the server's open list is the remaining truth
 * — anything still queued there keeps its chip) and clear any session-scoped
 * permission / external-input notices. Returns only the changed slices (empty
 * object when nothing matched, so the caller can keep the identity-stable
 * `state`). Shared by `turn_completed` (the `Stop` hook) and `turn_interrupted`
 * (the transcript-detected interrupt), which can occasionally both arrive, so
 * the drain is idempotent (a no-match is a no-op).
 */
function endTurnForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  const next: Partial<LiveState> = {};
  const remaining = Object.values(state.localSends).filter(
    (send) => send.sessionId !== sessionId,
  );
  if (remaining.length !== Object.keys(state.localSends).length) {
    next.localSends = Object.fromEntries(
      remaining.map((send) => [send.sendId, send]),
    );
  }
  if (state.activeTurns[sessionId]) {
    const activeTurns = { ...state.activeTurns };
    delete activeTurns[sessionId];
    next.activeTurns = activeTurns;
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
  sending: [],
  localSends: {},
  spawns: [],
  activeTurns: {},
  permission: {},
  unread: {},
  externalInput: {},
  resumeUnavailable: {},

  setConnection: (status) => set({ connection: status }),

  beginSending: (item) =>
    set((state) => ({ sending: [...state.sending, item] })),

  failSending: (id) =>
    set((state) => ({
      sending: state.sending.map((item) =>
        item.id === id ? { ...item, status: 'failed' } : item,
      ),
    })),

  removeSending: (id) =>
    set((state) => ({
      sending: state.sending.filter((item) => item.id !== id),
    })),

  recordLocalSend: (send) =>
    set((state) => ({
      localSends: { ...state.localSends, [send.sendId]: send },
    })),

  trackSpawn: (spawn) =>
    set((state) => ({
      spawns: [...state.spawns, { ...spawn, status: 'spawning' }],
    })),

  clearSpawn: (sessionId) =>
    set((state) => ({
      spawns: state.spawns.filter((spawn) => spawn.sessionId !== sessionId),
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

  resetTurnEphemera: () => set({ localSends: {}, activeTurns: {} }),

  seedActiveTurn: (sessionId, turn) =>
    set((state) => {
      if (turn.state !== 'in_flight' || state.activeTurns[sessionId]) {
        return state;
      }
      return { activeTurns: { ...state.activeTurns, [sessionId]: true } };
    }),

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
        case 'turn_started':
          // The send correlated with its transcript line and the turn is
          // confirmed in flight. The chip itself follows the send (server
          // list + localSends); here only the per-session running flag moves.
          return state.activeTurns[event.session_id]
            ? state
            : {
                activeTurns: { ...state.activeTurns, [event.session_id]: true },
              };
        case 'turn_completed': {
          // The turn ended: any permission prompt that was blocking THIS
          // session is resolved, any external-input notice has served its
          // purpose, and the session's tracked local sends are drained — the
          // server's open-send list (refetched by the router) is the
          // remaining truth for anything still queued. Scoped by session so a
          // turn in one session never drains another session's chips.
          const next = endTurnForSession(state, event.session_id);
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'turn_interrupted': {
          // The user interrupted the in-flight turn (Escape / Ctrl-C).
          // Claude's `Stop` hook does not fire on interrupt, so
          // `turn_completed` never arrives; the backend detects the interrupt
          // from the transcript and emits this hook-independent signal. Drain
          // exactly as a completed turn would.
          const next = endTurnForSession(state, event.session_id);
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'permission_requested':
          return {
            permission: {
              ...state.permission,
              [event.session_id]: {
                requestId: event.request_id,
                toolName: event.tool_name,
                toolInput: event.tool_input,
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
        case 'spawn_failed': {
          // The spawn never bound and the server reaped it (the row is gone).
          // Flip the tracked spawn to `failed` so the recoverable chip with
          // Retry / Dismiss surfaces, and drop any tracked local send for it —
          // its turn will never end. The event carries the REAL session id the
          // POST response returned, so this is an exact match; an untracked id
          // (e.g. another client's spawn) is a no-op.
          const idx = state.spawns.findIndex(
            (spawn) =>
              spawn.sessionId === event.session_id &&
              spawn.status === 'spawning',
          );
          if (idx === -1) {
            return state;
          }
          const spawns = state.spawns.slice();
          spawns[idx] = { ...spawns[idx], status: 'failed' };
          const next: Partial<LiveState> = { spawns };
          const remaining = Object.values(state.localSends).filter(
            (send) => send.sessionId !== event.session_id,
          );
          if (remaining.length !== Object.keys(state.localSends).length) {
            next.localSends = Object.fromEntries(
              remaining.map((send) => [send.sendId, send]),
            );
          }
          return next;
        }
        case 'external_input':
          // The external-input marker is session-scoped and only meaningful for
          // the focused session, so the router (`applySessionEvent`) records it
          // via `noteExternalInput` under a focus guard. Nothing to do here.
          return state;
        case 'session_registered':
          // Open/closed lifecycle is reflected by the sessions query, and the
          // tracked spawn is cleared by the workspace once it can focus the
          // freshly-listed session (it needs the id until then).
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
          // Closed state itself is reflected by the sessions query. But a
          // closed session has no live process, so its turn (if any) is over
          // and any permission prompt or stale external-input notice for it is
          // moot — drain exactly as a turn end would.
          const next = endTurnForSession(state, event.session_id);
          return Object.keys(next).length > 0 ? next : state;
        }
        default:
          return state;
      }
    }),
}));
