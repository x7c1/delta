import { create } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { PendingPermission, SessionEvent, Turn } from '@delta/wire-gen';
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
 *
 * Per-session notices (the permission prompt, the external-input marker, the
 * resume-impossible flag, the buffered early spawn failure) live in one
 * {@link LiveState.notices} map as a discriminated union, with each kind's
 * clearing rule declared in {@link NOTICE_LIFECYCLE} — one add path, one clear
 * path, instead of a separate `Record<SessionId, …>` per kind.
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

/**
 * The provisional live preview of an in-flight turn's assistant message,
 * accumulated from the `assistant_streaming` events the `MessageDisplay` hook
 * produces. Shown as a live bubble at the conversation tail while the turn
 * generates — including an assistant's pre-tool preamble, visible before the
 * user answers a blocking tool prompt.
 *
 * It is NOT a REST resource: the deltas carry no transcript id, so this cannot
 * be id-joined to the eventually-persisted message. It is reconciled per turn —
 * cleared on `turn_completed` / `turn_interrupted` / `session_closed` (and on a
 * reconnect, see {@link LiveState.resetTurnEphemera}), after which the persisted
 * assistant message renders via the normal transcript pipeline.
 */
export interface StreamingMessage {
  /** The hook's display-message id (not a transcript id). */
  messageId: string;
  /** The in-flight turn's thread, so the bubble only shows on its own thread. */
  threadId: ThreadId;
  /** The accumulated visible text so far (chunks joined in index order). */
  text: string;
  /** True once the final delta has arrived. */
  done: boolean;
  /**
   * The chunks received so far, keyed by `index`. Kept so out-of-order or
   * duplicate deliveries reconcile deterministically — {@link text} is always
   * recomputed by joining these in ascending index order.
   */
  chunks: Record<number, string>;
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

/**
 * A pending permission prompt blocking its session until it is answered — in
 * the browser (the notice's Allow/Deny) or in the terminal. The focused
 * session's notice drives the floating card over the transcript, and any
 * session's drives a badge on its navigator row.
 *
 * Set by `permission_requested` and re-seeded from the sends envelope's
 * `permission` field after a reconnect (the event is not replayed). Removed on
 * `permission_resolved` (same request only), when the turn ends, and when the
 * session closes. A user dismiss only flags the entry {@link dismissed} —
 * removing it would let the next refetch re-seed the same still-pending
 * request and resurrect the card the user just closed.
 */
export interface PermissionNotice {
  kind: 'permission';
  requestId: number;
  toolName: string;
  /** The tool input, serialized as JSON text (shown summarized). */
  toolInput: string;
  /** True once the user dismissed the card; the entry stays for de-dup. */
  dismissed: boolean;
}

/**
 * Someone typed straight into the session's embedded terminal (rather than
 * sending through the composer); surfaces an inline notice above the
 * composer. Recorded by the router under a focus guard, removed on dismiss,
 * when the turn ends, and when the session closes — otherwise the notice
 * would linger forever once shown. The retained `threadId` lets the
 * transcript pane gate visibility to the focused thread.
 */
export interface ExternalInputNotice {
  kind: 'external_input';
  threadId: ThreadId;
  prompt: string;
  at: number;
}

/**
 * A Send/open just failed to resume this session because its transcript is
 * gone (the server's `resume_unavailable`). Drives the inline "cannot be
 * resumed" notice; the session stays closed and no optimistic pending chip is
 * shown. Survives turn ends and closes (the session is already closed);
 * removed when the session opens after all.
 */
export interface ResumeUnavailableNotice {
  kind: 'resume_unavailable';
}

/**
 * A `spawn_failed` that arrived before {@link LiveState.trackSpawn} registered
 * the spawn. The event is broadcast on the live channel while the
 * `POST /api/sends` response travels back separately, so the failure can
 * legitimately outrun the registration; dropping it would leave the chip
 * spinning forever. Never rendered: buffered here and consumed by
 * {@link LiveState.trackSpawn}, which then registers the spawn as `failed`
 * directly. An entry for a spawn this client never tracks (e.g. another
 * client's) is removed if the session registers, and is otherwise inert.
 */
export interface SpawnFailureBufferedNotice {
  kind: 'spawn_failure_buffered';
}

/** One per-session notice; at most one of each kind exists per session. */
export type SessionNotice =
  | PermissionNotice
  | ExternalInputNotice
  | ResumeUnavailableNotice
  | SpawnFailureBufferedNotice;

export type SessionNoticeKind = SessionNotice['kind'];

/**
 * The lifecycle moments a notice kind can subscribe to for clearing. Explicit
 * dismissal and event-specific removals (`permission_resolved`) are handled
 * separately; this table covers the session-lifecycle sweeps so adding a
 * notice kind means declaring its policy here instead of threading a new map
 * through every event handler.
 *
 * - `turn_end` — `turn_completed` / `turn_interrupted` for the session.
 * - `session_closed` — the session's pane went away (its turn is over too).
 * - `session_opened` — the session resumed successfully.
 * - `session_registered` — the spawned session bound and became listable.
 */
type NoticeClearTrigger =
  | 'turn_end'
  | 'session_closed'
  | 'session_opened'
  | 'session_registered';

const NOTICE_LIFECYCLE: Record<
  SessionNoticeKind,
  readonly NoticeClearTrigger[]
> = {
  // A pending prompt blocks its turn, so the turn ending (or the session
  // closing — no live process, no dialog) means the question is moot.
  permission: ['turn_end', 'session_closed'],
  // Same scope as the permission prompt: once the turn it interleaved with is
  // over (or the session is gone), the marker has served its purpose.
  external_input: ['turn_end', 'session_closed'],
  // A resume-impossible session is closed and turn-less; only a successful
  // open proves the flag stale.
  resume_unavailable: ['session_opened'],
  // A registered session can no longer fail to spawn, so a buffered failure
  // for it (necessarily foreign — ids are unique) is stale.
  spawn_failure_buffered: ['session_registered'],
};

/**
 * The notice of `kind` for a session, narrowed to its union member, or `null`.
 */
export function noticeOf<K extends SessionNoticeKind>(
  notices: Record<SessionId, SessionNotice[]>,
  sessionId: SessionId,
  kind: K,
): Extract<SessionNotice, { kind: K }> | null {
  const found = (notices[sessionId] ?? []).find(
    (notice) => notice.kind === kind,
  );
  return (found as Extract<SessionNotice, { kind: K }> | undefined) ?? null;
}

/** Upsert a session's notice of its kind (at most one per kind). */
function withNotice(
  notices: Record<SessionId, SessionNotice[]>,
  sessionId: SessionId,
  notice: SessionNotice,
): Record<SessionId, SessionNotice[]> {
  const rest = (notices[sessionId] ?? []).filter(
    (existing) => existing.kind !== notice.kind,
  );
  return { ...notices, [sessionId]: [...rest, notice] };
}

/**
 * Remove a session's notices matching `predicate`, dropping the session's
 * (then empty) list entirely. Returns the changed slice, or an empty object
 * when nothing matched so callers can keep the identity-stable state.
 */
function removeNotices(
  notices: Record<SessionId, SessionNotice[]>,
  sessionId: SessionId,
  predicate: (notice: SessionNotice) => boolean,
): { notices: Record<SessionId, SessionNotice[]> } | Record<string, never> {
  const current = notices[sessionId] ?? [];
  const remaining = current.filter((notice) => !predicate(notice));
  if (remaining.length === current.length) {
    return {};
  }
  const next = { ...notices };
  if (remaining.length === 0) {
    delete next[sessionId];
  } else {
    next[sessionId] = remaining;
  }
  return { notices: next };
}

/** Apply one lifecycle trigger to a session's notices via the policy table. */
function clearNoticesOn(
  notices: Record<SessionId, SessionNotice[]>,
  sessionId: SessionId,
  trigger: NoticeClearTrigger,
): { notices: Record<SessionId, SessionNotice[]> } | Record<string, never> {
  return removeNotices(notices, sessionId, (notice) =>
    NOTICE_LIFECYCLE[notice.kind].includes(trigger),
  );
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
   * Per-session notices, at most one per {@link SessionNoticeKind} per
   * session. Read through {@link noticeOf}; lifecycle clearing follows
   * {@link NOTICE_LIFECYCLE}.
   */
  notices: Record<SessionId, SessionNotice[]>;
  /** Unread counts keyed by thread id; cleared when a thread becomes active. */
  unread: Record<ThreadId, number>;
  /**
   * The provisional live preview of each session's in-flight assistant message,
   * keyed by session id. Appended by `assistant_streaming` and cleared on turn
   * end / close / reconnect (see {@link StreamingMessage}). At most one per
   * session — `claude` streams one message at a time.
   */
  streamingMessages: Record<SessionId, StreamingMessage>;

  setConnection: (status: ConnectionStatus) => void;
  /** Record a submit whose `POST /api/sends` is about to fly. */
  beginSending: (item: SendingItem) => void;
  /** Mark an in-flight submit rejected, surfacing the recoverable chip. */
  failSending: (id: string) => void;
  /** Drop a submit chip (POST accepted, or the failure dismissed). */
  removeSending: (id: string) => void;
  /** Track an accepted send until its turn ends (real ids from the POST). */
  recordLocalSend: (send: LocalSend) => void;
  /**
   * Track a new-session spawn (real ids from the POST response). If the
   * spawn's failure already arrived (see {@link SpawnFailureBufferedNotice}),
   * the spawn is registered as `failed` immediately.
   */
  trackSpawn: (spawn: Omit<SpawnItem, 'status'>) => void;
  /** Drop a tracked spawn (it registered, or its failure was dismissed). */
  clearSpawn: (sessionId: SessionId) => void;
  /** Flag a session as resume-impossible, surfacing the inline notice. */
  markResumeUnavailable: (sessionId: SessionId) => void;
  /** Clear a session's resume-impossible flag (e.g. once it opens). */
  clearResumeUnavailable: (sessionId: SessionId) => void;
  /**
   * Drop the event-reconstructed turn-scoped state: tracked local sends, the
   * active-turn flags, and the permission notices. Used on a live-stream
   * reconnect: the turn-end / `permission_resolved` events that would have
   * drained these were broadcast while the socket was down and are not
   * replayed, so they can no longer be reconciled from events. All three
   * recover from the refetched sends envelope — the open-send list by
   * refetch, the active-turn flag via {@link seedActiveTurn}, and the
   * permission notice via {@link seedPermission}. Other notice kinds stay:
   * they cannot be re-seeded, and each has a non-event escape hatch (a user
   * dismiss or a lifecycle trigger).
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
  /**
   * Seed a session's permission notice from the server's queryable pending
   * dialog (the `permission` field of `GET /api/sessions/{id}/sends`).
   * Mirrors {@link seedActiveTurn}: set-only (`null` clears nothing —
   * clearing is owned by the events and the lifecycle sweeps), and a report
   * of the request the notice already shows changes nothing, so a refetch can
   * neither resurrect a notice an event just resolved nor un-dismiss one the
   * user closed.
   */
  seedPermission: (
    sessionId: SessionId,
    permission: PendingPermission | null,
  ) => void;
  bumpUnread: (threadId: ThreadId) => void;
  clearUnread: (threadId: ThreadId) => void;
  /** Record an external (direct-pane) input notice for a session/thread. */
  noteExternalInput: (
    sessionId: SessionId,
    threadId: ThreadId,
    prompt: string,
  ) => void;
  /** Dismiss the permission notice for a session (kept, flagged dismissed). */
  dismissPermission: (sessionId: SessionId) => void;
  /** Dismiss the external-input notice for a session. */
  dismissExternalInput: (sessionId: SessionId) => void;
  /**
   * Apply a live session event, mutating only session-scoped ephemeral state
   * (turn tracking, the spawn registry, the permission notice). Focus-dependent
   * signals (the external-input notice, unread badges) are recorded by the
   * router under a focus guard, not here.
   */
  applyEvent: (event: SessionEvent) => void;
}

/**
 * Drop the tracked local sends of one session, returning the changed slice
 * (empty object when nothing matched). Used when the session's spawn failed —
 * its turn will never end, so nothing else drains those sends.
 */
function dropLocalSendsForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  const remaining = Object.values(state.localSends).filter(
    (send) => send.sessionId !== sessionId,
  );
  if (remaining.length === Object.keys(state.localSends).length) {
    return {};
  }
  return {
    localSends: Object.fromEntries(remaining.map((send) => [send.sendId, send])),
  };
}

/**
 * Drop the streaming preview of one session, returning the changed slice (empty
 * object when none existed). Used when the turn ends — the persisted assistant
 * message then renders via the normal pipeline — and on a reconnect.
 */
function dropStreamingForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  if (!state.streamingMessages[sessionId]) {
    return {};
  }
  const streamingMessages = { ...state.streamingMessages };
  delete streamingMessages[sessionId];
  return { streamingMessages };
}

/**
 * Compute the state changes for a turn ending in `sessionId`: drop the tracked
 * local sends for that session (the server's open list is the remaining truth
 * — anything still queued there keeps its chip), clear the running flag, and
 * sweep the turn-scoped notices (see {@link NOTICE_LIFECYCLE}). Returns only
 * the changed slices (empty object when nothing matched, so the caller can
 * keep the identity-stable `state`). Shared by `turn_completed` (the `Stop`
 * hook), `turn_interrupted` (the transcript-detected interrupt) — which can
 * occasionally both arrive, so the drain is idempotent — and `session_closed`
 * (a closed session has no live process, so its turn is over too).
 */
function endTurnForSession(
  state: LiveState,
  sessionId: SessionId,
  trigger: 'turn_end' | 'session_closed',
): Partial<LiveState> {
  const next: Partial<LiveState> = dropLocalSendsForSession(state, sessionId);
  if (state.activeTurns[sessionId]) {
    const activeTurns = { ...state.activeTurns };
    delete activeTurns[sessionId];
    next.activeTurns = activeTurns;
  }
  return {
    ...next,
    ...dropStreamingForSession(state, sessionId),
    ...clearNoticesOn(state.notices, sessionId, trigger),
  };
}

export const useLiveStore = create<LiveState>((set) => ({
  connection: 'connecting',
  sending: [],
  localSends: {},
  spawns: [],
  activeTurns: {},
  notices: {},
  unread: {},
  streamingMessages: {},

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
    set((state) => {
      if (!noticeOf(state.notices, spawn.sessionId, 'spawn_failure_buffered')) {
        return { spawns: [...state.spawns, { ...spawn, status: 'spawning' }] };
      }
      // The failure outran the POST response: register the spawn already
      // failed (the Retry/Dismiss chip surfaces right away), consume the
      // buffered failure, and drop the just-recorded local send for it — its
      // turn will never end.
      return {
        spawns: [...state.spawns, { ...spawn, status: 'failed' }],
        ...removeNotices(
          state.notices,
          spawn.sessionId,
          (notice) => notice.kind === 'spawn_failure_buffered',
        ),
        ...dropLocalSendsForSession(state, spawn.sessionId),
      };
    }),

  clearSpawn: (sessionId) =>
    set((state) => ({
      spawns: state.spawns.filter((spawn) => spawn.sessionId !== sessionId),
    })),

  markResumeUnavailable: (sessionId) =>
    set((state) =>
      noticeOf(state.notices, sessionId, 'resume_unavailable')
        ? state
        : {
            notices: withNotice(state.notices, sessionId, {
              kind: 'resume_unavailable',
            }),
          },
    ),

  clearResumeUnavailable: (sessionId) =>
    set((state) => {
      const next = removeNotices(
        state.notices,
        sessionId,
        (notice) => notice.kind === 'resume_unavailable',
      );
      return Object.keys(next).length > 0 ? next : state;
    }),

  resetTurnEphemera: () =>
    set((state) => {
      const notices: Record<SessionId, SessionNotice[]> = {};
      for (const [sessionId, list] of Object.entries(state.notices)) {
        const remaining = list.filter(
          (notice) => notice.kind !== 'permission',
        );
        if (remaining.length > 0) {
          notices[sessionId] = remaining;
        }
      }
      // The live previews' turn-end clears may also have been missed during the
      // outage and cannot be recovered (no re-seed of a partial stream this
      // PR), so drop them too — the flushed message renders from the refetch.
      return { localSends: {}, activeTurns: {}, notices, streamingMessages: {} };
    }),

  seedActiveTurn: (sessionId, turn) =>
    set((state) => {
      if (turn.state !== 'in_flight' || state.activeTurns[sessionId]) {
        return state;
      }
      return { activeTurns: { ...state.activeTurns, [sessionId]: true } };
    }),

  seedPermission: (sessionId, permission) =>
    set((state) => {
      if (permission === null) {
        return state;
      }
      const current = noticeOf(state.notices, sessionId, 'permission');
      if (current?.requestId === permission.request_id) {
        return state;
      }
      return {
        notices: withNotice(state.notices, sessionId, {
          kind: 'permission',
          requestId: permission.request_id,
          toolName: permission.tool_name,
          toolInput: permission.tool_input,
          dismissed: false,
        }),
      };
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
      notices: withNotice(state.notices, sessionId, {
        kind: 'external_input',
        threadId,
        prompt,
        at: Date.now(),
      }),
    })),

  dismissPermission: (sessionId) =>
    set((state) => {
      const current = noticeOf(state.notices, sessionId, 'permission');
      if (!current || current.dismissed) {
        return state;
      }
      // Keep the entry, flagged: removal would let the next sends refetch
      // re-seed the same still-pending request and resurrect the card.
      return {
        notices: withNotice(state.notices, sessionId, {
          ...current,
          dismissed: true,
        }),
      };
    }),

  dismissExternalInput: (sessionId) =>
    set((state) => {
      const next = removeNotices(
        state.notices,
        sessionId,
        (notice) => notice.kind === 'external_input',
      );
      return Object.keys(next).length > 0 ? next : state;
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
          // The turn ended: the session's tracked local sends are drained —
          // the server's open-send list (refetched by the router) is the
          // remaining truth for anything still queued — and the turn-scoped
          // notices are swept (see NOTICE_LIFECYCLE). Scoped by session so a
          // turn in one session never drains another session's chips.
          const next = endTurnForSession(state, event.session_id, 'turn_end');
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'turn_interrupted': {
          // The user interrupted the in-flight turn (Escape / Ctrl-C).
          // Claude's `Stop` hook does not fire on interrupt, so
          // `turn_completed` never arrives; the backend detects the interrupt
          // from the transcript and emits this hook-independent signal. Drain
          // exactly as a completed turn would.
          const next = endTurnForSession(state, event.session_id, 'turn_end');
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'permission_requested':
          return {
            notices: withNotice(state.notices, event.session_id, {
              kind: 'permission',
              requestId: event.request_id,
              toolName: event.tool_name,
              toolInput: event.tool_input,
              dismissed: false,
            }),
          };
        case 'permission_resolved': {
          // The request was answered (a browser decision or the correlated
          // tool_result). Remove the notice only when it is the SAME request
          // that resolved, so a stale resolution never wipes a newer pending
          // prompt for the same session. An auto-approved tool resolves
          // almost immediately, so this clears the brief notice; a genuine
          // prompt has no resolution until the human answers.
          const next = removeNotices(
            state.notices,
            event.session_id,
            (notice) =>
              notice.kind === 'permission' &&
              notice.requestId === event.request_id,
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'spawn_failed': {
          // The spawn never bound and the server reaped it (the row is gone).
          // Flip the tracked spawn to `failed` so the recoverable chip with
          // Retry / Dismiss surfaces, and drop any tracked local send for it —
          // its turn will never end. The event carries the REAL session id the
          // POST response returned, so this is an exact match. An id with no
          // tracked spawn at all is buffered, NOT dropped: the broadcast can
          // outrun this client's own POST response, in which case `trackSpawn`
          // consumes the buffer moments later (a genuinely foreign id — e.g.
          // another client's spawn — leaves an inert entry).
          const idx = state.spawns.findIndex(
            (spawn) =>
              spawn.sessionId === event.session_id &&
              spawn.status === 'spawning',
          );
          if (idx === -1) {
            const alreadyTracked = state.spawns.some(
              (spawn) => spawn.sessionId === event.session_id,
            );
            // A duplicate event for an already-failed spawn changes nothing.
            return alreadyTracked
              ? state
              : {
                  notices: withNotice(state.notices, event.session_id, {
                    kind: 'spawn_failure_buffered',
                  }),
                };
          }
          const spawns = state.spawns.slice();
          spawns[idx] = { ...spawns[idx], status: 'failed' };
          return {
            spawns,
            ...dropLocalSendsForSession(state, event.session_id),
          };
        }
        case 'assistant_streaming': {
          // A chunk of the in-flight turn's assistant message arrived. Append
          // it to the session's live preview (a new message_id, or the first
          // chunk after a turn end cleared the buffer, starts fresh). Chunks
          // are kept by index and the text recomputed by joining them in
          // ascending order, so out-of-order or duplicate deliveries reconcile
          // deterministically. Cleared per turn by the turn-end events.
          const prev = state.streamingMessages[event.session_id];
          const chunks =
            prev && prev.messageId === event.message_id
              ? { ...prev.chunks, [event.index]: event.delta }
              : { [event.index]: event.delta };
          const text = Object.keys(chunks)
            .map(Number)
            .sort((a, b) => a - b)
            .map((index) => chunks[index])
            .join('');
          return {
            streamingMessages: {
              ...state.streamingMessages,
              [event.session_id]: {
                messageId: event.message_id,
                threadId: event.thread_id,
                text,
                done: event.final,
                chunks,
              },
            },
          };
        }
        case 'external_input':
          // The external-input notice is session-scoped and only meaningful
          // for the focused session, so the router (`applySessionEvent`)
          // records it via `noteExternalInput` under a focus guard. Nothing
          // to do here.
          return state;
        case 'session_registered': {
          // Open/closed lifecycle is reflected by the sessions query, and the
          // tracked spawn is cleared by the workspace once it can focus the
          // freshly-listed session (it needs the id until then). The notice
          // sweep drops a stale buffered spawn failure (see NOTICE_LIFECYCLE).
          const next = clearNoticesOn(
            state.notices,
            event.session_id,
            'session_registered',
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'session_opened': {
          // The session resumed successfully; the sweep drops a stale "cannot
          // be resumed" notice. Open/closed itself is reflected by the
          // sessions query, not ephemeral here.
          const next = clearNoticesOn(
            state.notices,
            event.session_id,
            'session_opened',
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'session_closed': {
          // Closed state itself is reflected by the sessions query. But a
          // closed session has no live process, so its turn (if any) is over
          // and its turn-scoped notices are moot — drain exactly as a turn
          // end would.
          const next = endTurnForSession(
            state,
            event.session_id,
            'session_closed',
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        default:
          return state;
      }
    }),
}));
