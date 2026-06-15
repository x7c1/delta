import { create } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  PendingPermission,
  PendingQuestion,
  RunningSubagent,
  SessionEvent,
  Turn,
} from '@delta/wire-gen';
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
   * composer (which retains the chosen `workdir` and selected launch options
   * so a failed launch request can be retried with the same configuration).
   */
  target:
    | { kind: 'thread'; sessionId: SessionId; threadId: ThreadId }
    | {
        kind: 'new-session';
        workdir: string | null;
        launchOptionIds: number[];
      };
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

/**
 * A subagent (the `Agent`/`Task` tool) currently running inside a session's
 * turn. A subagent runs in its own transcript that Delta never tails, so the
 * conversation pane shows nothing while it works — this is the only live signal
 * that one is running, surfaced as a badge on the navigator row and an indicator
 * near the conversation tail.
 *
 * Added by `subagent_started`, removed by the matching `subagent_finished`
 * (correlated by {@link toolUseId}), and swept when the turn ends / the session
 * closes (a subagent cannot outlive its turn). Re-seeded from the sends
 * envelope's `running_subagents` after a reconnect, so a missed start/finish
 * event heals from a plain refetch.
 *
 * This is the FOREGROUND (synchronous) case only; background subagents
 * (`run_in_background: true`) complete via a different signal and are not yet
 * tracked.
 */
export interface SubagentActivity {
  /** The `Agent`/`Task` call's `tool_use_id` (its stable correlation key). */
  toolUseId: string;
  /** The subagent type (e.g. `general-purpose`), or null if none was given. */
  subagentType: string | null;
  /** The short task description for display, or null if none was given. */
  description: string | null;
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
  /** The selected launch-option ids, retained for the same Retry. */
  launchOptionIds: number[];
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
 * Claude Code's built-in `AskUserQuestion` tool is presenting a
 * multiple-choice question for the user to answer in the embedded terminal.
 * Drives the floating question card over the transcript (the readable question
 * + options), replacing the confusing generic Allow/Deny notice the tool used
 * to show. Answering from Delta is out of scope: the user picks in the TUI.
 *
 * Set by `question_asked` and re-seeded from the sends envelope's `question`
 * field after a reconnect (the event is not replayed). Removed on the matching
 * `permission_resolved` (the correlated tool_result resolved the question's
 * request row — the user answered), when the turn ends, and when the session
 * closes. A user dismiss only flags {@link dismissed}, mirroring
 * {@link PermissionNotice}, so a refetch cannot resurrect the just-closed card.
 */
export interface QuestionNotice {
  kind: 'question';
  requestId: number;
  /** The raw `{questions:[…]}` tool input, parsed by the card to render it. */
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
  | QuestionNotice
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
  // An AskUserQuestion blocks its turn exactly like a permission prompt, so it
  // shares the same backstop sweep (a question cannot outlive its turn).
  question: ['turn_end', 'session_closed'],
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
   * Sessions whose turn completed while the user was viewing a DIFFERENT
   * session, so its navigator row carries an unread dot until focused. Set by
   * {@link markSessionUnread} (called from the router only when the completing
   * session is not the focused one), cleared by {@link clearSessionUnread} when
   * the session is focused. A boolean flag, not a count: the dot only signals
   * "something finished here", it does not tally turns. In-memory only (resets
   * on reload), mirroring {@link unread} — persistence across reload would need
   * backend support and is out of scope. The running spinner takes precedence
   * in the row's rendering (a session running again shows the spinner, not a
   * stale dot), but the flag itself is left set so the dot reappears the moment
   * that turn ends off-focus; only focusing the session clears it.
   */
  unreadSessions: Record<SessionId, true>;
  /**
   * The provisional live preview of each session's in-flight assistant message,
   * keyed by session id. Appended by `assistant_streaming` and cleared on turn
   * end / close / reconnect (see {@link StreamingMessage}). At most one per
   * session — `claude` streams one message at a time.
   */
  streamingMessages: Record<SessionId, StreamingMessage>;
  /**
   * The subagents currently running in each session's turn, keyed by session id
   * and kept in start order. Added by `subagent_started`, removed by the
   * matching `subagent_finished`, swept on turn end / close, and re-seeded from
   * the sends envelope after a reconnect (see {@link SubagentActivity}). A
   * session with none running has no entry (the empty list is dropped).
   */
  runningSubagents: Record<SessionId, SubagentActivity[]>;
  /**
   * Per-session credit for a turn-end that arrived before the send it ended was
   * recorded, keyed by session id. A `turn_completed` / `turn_interrupted`
   * carries only a session id, and the only drain path for a tracked local send
   * is such a turn-end event (see {@link LiveState.localSends}). An echo turn
   * can complete in well under the time the `POST /api/sends` takes to resolve,
   * so under load the turn-end event can land BEFORE that POST's `onSuccess`
   * runs {@link LiveState.recordLocalSend}. The turn-end then drains nothing
   * (the send is not tracked yet), and the late `recordLocalSend` inserts a send
   * whose turn has already ended — a chip with no remaining drain trigger (it is
   * also absent from the server's open-send list, so a refetch never re-includes
   * it), lingering forever. {@link endTurnForSession} grants a credit here when a
   * turn-end drains no local send yet still has an in-flight submit on the same
   * session (the racing POST), and {@link recordLocalSend} consumes one to drop
   * the just-ended send instead of tracking it. A counter, not a flag: two sends
   * could be in flight at once. Cleared per session as it drains to zero.
   */
  endedBeforeRecorded: Record<SessionId, number>;

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
   * active-turn flags, and the permission/question notices. Used on a
   * live-stream reconnect: the turn-end / `permission_resolved` events that
   * would have drained these were broadcast while the socket was down and are
   * not replayed, so they can no longer be reconciled from events. They all
   * recover from the refetched sends envelope — the open-send list by refetch,
   * the active-turn flag via {@link seedActiveTurn}, the permission notice via
   * {@link seedPermission}, and the question notice via {@link seedQuestion}.
   * Other notice kinds stay: they cannot be re-seeded, and each has a
   * non-event escape hatch (a user dismiss or a lifecycle trigger).
   */
  resetTurnEphemera: () => void;
  /**
   * Seed a session's active-turn flag from the server's queryable turn state
   * (the `turn` field of `GET /api/sessions/{id}/sends`).
   *
   * Two modes, picked by `authoritative`:
   *
   * - `authoritative: false` (the default — a possibly-stale read): set-only,
   *   and only for `in_flight` — the phase `turn_started` would have announced.
   *   This heals a reconnect that missed `turn_started`: it re-sets a dropped
   *   flag without ever letting a momentarily-stale `idle` refetch wipe a flag
   *   a live event just set. `awaiting_echo` is a dispatch whose turn has not
   *   started yet (exactly like a live `send_dispatched`), and `idle` changes
   *   nothing.
   * - `authoritative: true` (a genuinely fresh fetch that has settled): the
   *   server is the source of truth, so reconcile the flag to match —
   *   `activeTurns[sessionId] = (turn.state === 'in_flight')`. A fresh `idle`
   *   authoritatively means "no running turn" and CLEARS the flag; a fresh
   *   `in_flight` keeps/re-sets it (so reconnect healing still works when the
   *   resync refetch lands `in_flight`). `awaiting_echo` reconciles to
   *   not-running, consistent with the set-only mode ignoring it.
   *
   * The authoritative mode exists to clear a flag the stale-cache read would
   * otherwise resurrect: after a turn completes off-focus its `turn_completed`
   * clears `activeTurns`, but re-focusing the session serves the stale cached
   * `in_flight` envelope before the refetch — without an authoritative clear on
   * the following fresh `idle`, the set-only re-seed would leave the spinner
   * stuck on. Callers must therefore pass `authoritative: true` only for a read
   * known to be fresh (the query settled, not a stale-cache placeholder shown
   * mid-refetch).
   */
  seedActiveTurn: (
    sessionId: SessionId,
    turn: Turn,
    authoritative: boolean,
  ) => void;
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
  /**
   * Seed a session's question notice from the server's queryable pending
   * question (the `question` field of `GET /api/sessions/{id}/sends`). Mirrors
   * {@link seedPermission}: set-only (`null` clears nothing), and a report of
   * the request the card already shows changes nothing, so a refetch can
   * neither resurrect a card an event just resolved nor un-dismiss one the
   * user closed.
   */
  seedQuestion: (
    sessionId: SessionId,
    question: PendingQuestion | null,
  ) => void;
  /**
   * Seed a session's running-subagent set from the server's queryable list
   * (the `running_subagents` field of `GET /api/sessions/{id}/sends`).
   *
   * Unlike the permission/question seeds (which are set-only because a notice
   * can be user-dismissed), the running set carries no per-entry user state, so
   * the server list is authoritative: it REPLACES the session's set, healing a
   * reconnect that missed a `subagent_started` (re-adds it) or a
   * `subagent_finished` (drops it). An empty list clears the session's entry.
   */
  seedRunningSubagents: (
    sessionId: SessionId,
    running: RunningSubagent[],
  ) => void;
  bumpUnread: (threadId: ThreadId) => void;
  clearUnread: (threadId: ThreadId) => void;
  /**
   * Flag a session unread (its turn completed off-focus). Idempotent: a flag
   * already set changes nothing. The router gates the focus check, so this only
   * ever marks a non-focused session.
   */
  markSessionUnread: (sessionId: SessionId) => void;
  /** Clear a session's unread flag (it became the focused session). */
  clearSessionUnread: (sessionId: SessionId) => void;
  /** Record an external (direct-pane) input notice for a session/thread. */
  noteExternalInput: (
    sessionId: SessionId,
    threadId: ThreadId,
    prompt: string,
  ) => void;
  /** Dismiss the permission notice for a session (kept, flagged dismissed). */
  dismissPermission: (sessionId: SessionId) => void;
  /** Dismiss the question notice for a session (kept, flagged dismissed). */
  dismissQuestion: (sessionId: SessionId) => void;
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
 * Drop the running-subagent set of one session, returning the changed slice
 * (empty object when none existed). Used when the turn ends — a subagent cannot
 * outlive the turn that spawned it — and on a reconnect.
 */
function dropRunningSubagentsForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  if (!state.runningSubagents[sessionId]) {
    return {};
  }
  const runningSubagents = { ...state.runningSubagents };
  delete runningSubagents[sessionId];
  return { runningSubagents };
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
 *
 * The streaming preview is deliberately NOT dropped here. At a normal turn
 * completion the bubble must stay until the persisted assistant message lands
 * in the transcript refetch, at which point the content-based suppression guard
 * (`persistedHasStreamedText`) removes it in the SAME render that adds the
 * persisted copy — a seamless swap with no empty gap. Dropping the buffer on
 * `turn_completed` (which can outrun the async refetch) is exactly what opened
 * that gap. The buffer is harmless between turns: it sits invisibly suppressed
 * and is overwritten by the next turn's first `assistant_streaming` (a new
 * `message_id` starts fresh). Callers that end a turn WITHOUT a guaranteed
 * matching persisted message — `turn_interrupted` (a partial may never persist)
 * and `session_closed` (no live process) — pass {@link dropStreaming} so the
 * dangling preview is cleared explicitly.
 */
function endTurnForSession(
  state: LiveState,
  sessionId: SessionId,
  trigger: 'turn_end' | 'session_closed',
  dropStreaming: boolean,
): Partial<LiveState> {
  const next: Partial<LiveState> = dropLocalSendsForSession(state, sessionId);
  // A turn ended but drained no tracked local send, yet a submit on this
  // session is still mid-POST: the turn-end raced ahead of that POST's
  // `onSuccess`. Credit the session so the imminent `recordLocalSend` drops the
  // already-ended send instead of leaving a chip with no future drain trigger.
  // Gated on an in-flight submit so a normal already-drained turn-end (or a
  // direct-pane turn with no browser submit) grants nothing. `session_closed`
  // is excluded: a closed session accepts no further sends, so no late
  // `recordLocalSend` can follow. The credit is always consumed and never
  // leaks: the server runs a turn only for an ACCEPTED send, so a turn-end
  // implies its POST returned 2xx, whose `onSuccess` is the very
  // `recordLocalSend` that consumes the credit. A rejected POST has no turn,
  // so it cannot have raced a turn-end here.
  if (
    trigger === 'turn_end' &&
    next.localSends === undefined &&
    state.sending.some(
      (item) =>
        item.target.kind === 'thread' &&
        item.target.sessionId === sessionId &&
        item.status === 'sending',
    )
  ) {
    next.endedBeforeRecorded = {
      ...state.endedBeforeRecorded,
      [sessionId]: (state.endedBeforeRecorded[sessionId] ?? 0) + 1,
    };
  }
  if (state.activeTurns[sessionId]) {
    const activeTurns = { ...state.activeTurns };
    delete activeTurns[sessionId];
    next.activeTurns = activeTurns;
  }
  return {
    ...next,
    ...(dropStreaming ? dropStreamingForSession(state, sessionId) : {}),
    // A subagent cannot outlive the turn that spawned it, so any still-running
    // entry is cleared whenever the turn ends (or the session closes). This
    // also covers a foreground `subagent_finished` that was missed.
    ...dropRunningSubagentsForSession(state, sessionId),
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
  unreadSessions: {},
  streamingMessages: {},
  runningSubagents: {},
  endedBeforeRecorded: {},

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
    set((state) => {
      // The send's turn already ended before this POST `onSuccess` ran (the
      // turn-end event raced ahead under load and credited the session — see
      // {@link LiveState.endedBeforeRecorded}). Consume the credit and drop the
      // send: tracking it now would leave a permanently undrainable chip.
      const credit = state.endedBeforeRecorded[send.sessionId] ?? 0;
      if (credit > 0) {
        const endedBeforeRecorded = { ...state.endedBeforeRecorded };
        if (credit > 1) {
          endedBeforeRecorded[send.sessionId] = credit - 1;
        } else {
          delete endedBeforeRecorded[send.sessionId];
        }
        return { endedBeforeRecorded };
      }
      return {
        localSends: { ...state.localSends, [send.sendId]: send },
      };
    }),

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
          (notice) => notice.kind !== 'permission' && notice.kind !== 'question',
        );
        if (remaining.length > 0) {
          notices[sessionId] = remaining;
        }
      }
      // The live previews' turn-end clears may also have been missed during the
      // outage and cannot be recovered (no re-seed of a partial stream this
      // PR), so drop them too — the flushed message renders from the refetch.
      return {
        localSends: {},
        activeTurns: {},
        notices,
        streamingMessages: {},
        // The running-subagent set is re-seeded authoritatively from the sends
        // envelope's `running_subagents` on the resync refetch (see
        // {@link seedRunningSubagents}), so drop the event-reconstructed copy —
        // a `subagent_started`/`subagent_finished` missed during the outage is
        // not replayed.
        runningSubagents: {},
        endedBeforeRecorded: {},
      };
    }),

  seedActiveTurn: (sessionId, turn, authoritative) =>
    set((state) => {
      const running = turn.state === 'in_flight';
      if (!authoritative) {
        // Possibly-stale read: set-only healing. Never clear from here —
        // turn-end events own clearing, so a stale `idle` cannot wipe a flag
        // a live event just set.
        if (!running || state.activeTurns[sessionId]) {
          return state;
        }
        return { activeTurns: { ...state.activeTurns, [sessionId]: true } };
      }
      // Fresh read: the server is authoritative, so reconcile to its truth.
      if (running === !!state.activeTurns[sessionId]) {
        return state;
      }
      const activeTurns = { ...state.activeTurns };
      if (running) {
        activeTurns[sessionId] = true;
      } else {
        delete activeTurns[sessionId];
      }
      return { activeTurns };
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

  seedQuestion: (sessionId, question) =>
    set((state) => {
      if (question === null) {
        return state;
      }
      const current = noticeOf(state.notices, sessionId, 'question');
      if (current?.requestId === question.request_id) {
        return state;
      }
      return {
        notices: withNotice(state.notices, sessionId, {
          kind: 'question',
          requestId: question.request_id,
          toolInput: question.tool_input,
          dismissed: false,
        }),
      };
    }),

  seedRunningSubagents: (sessionId, running) =>
    set((state) => {
      const current = state.runningSubagents[sessionId] ?? [];
      // Already in sync (same ids, same order): keep the identity-stable state.
      const same =
        current.length === running.length &&
        current.every((s, i) => s.toolUseId === running[i].tool_use_id);
      if (same) {
        return state;
      }
      const runningSubagents = { ...state.runningSubagents };
      if (running.length === 0) {
        delete runningSubagents[sessionId];
      } else {
        runningSubagents[sessionId] = running.map((s) => ({
          toolUseId: s.tool_use_id,
          subagentType: s.subagent_type,
          description: s.description,
        }));
      }
      return { runningSubagents };
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

  markSessionUnread: (sessionId) =>
    set((state) =>
      state.unreadSessions[sessionId]
        ? state
        : {
            unreadSessions: { ...state.unreadSessions, [sessionId]: true },
          },
    ),

  clearSessionUnread: (sessionId) =>
    set((state) => {
      if (!state.unreadSessions[sessionId]) {
        return state;
      }
      const next = { ...state.unreadSessions };
      delete next[sessionId];
      return { unreadSessions: next };
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

  dismissQuestion: (sessionId) =>
    set((state) => {
      const current = noticeOf(state.notices, sessionId, 'question');
      if (!current || current.dismissed) {
        return state;
      }
      // Keep the entry, flagged: removal would let the next sends refetch
      // re-seed the same still-pending question and resurrect the card.
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
          // turn in one session never drains another session's chips. The
          // streaming preview is left in place (dropStreaming: false): the
          // persisted message will suppress the bubble when it lands, a
          // gap-free swap (see endTurnForSession).
          const next = endTurnForSession(
            state,
            event.session_id,
            'turn_end',
            false,
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'turn_interrupted': {
          // The user interrupted the in-flight turn (Escape / Ctrl-C).
          // Claude's `Stop` hook does not fire on interrupt, so
          // `turn_completed` never arrives; the backend detects the interrupt
          // from the transcript and emits this hook-independent signal. Drain
          // exactly as a completed turn would, but also drop the streaming
          // preview (dropStreaming: true): an interrupted partial may have no
          // matching persisted message, so nothing else would clear it.
          const next = endTurnForSession(
            state,
            event.session_id,
            'turn_end',
            true,
          );
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
        case 'question_asked':
          // Claude Code's AskUserQuestion is presenting its options in the
          // TUI; surface the dedicated question card. Driven off PreToolUse so
          // the same `permission_resolved` (from the correlated tool_result)
          // clears it once the user answers.
          return {
            notices: withNotice(state.notices, event.session_id, {
              kind: 'question',
              requestId: event.request_id,
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
          // prompt has no resolution until the human answers. The same event
          // also clears a `question` notice with the matching request id: an
          // AskUserQuestion's request row resolves the moment its tool_result
          // (the user's pick) is ingested.
          const next = removeNotices(
            state.notices,
            event.session_id,
            (notice) =>
              (notice.kind === 'permission' || notice.kind === 'question') &&
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
        case 'subagent_started': {
          // A subagent (the `Agent`/`Task` tool) started in the main turn. It
          // runs in its own (untailed) transcript, so this is the only live
          // signal — add it to the session's running set so the navigator badge
          // and conversation indicator appear. Keyed by `tool_use_id`: a
          // duplicate start for an id already tracked changes nothing (a retried
          // event), and new entries append so the set stays in start order.
          const current = state.runningSubagents[event.session_id] ?? [];
          if (current.some((s) => s.toolUseId === event.tool_use_id)) {
            return state;
          }
          return {
            runningSubagents: {
              ...state.runningSubagents,
              [event.session_id]: [
                ...current,
                {
                  toolUseId: event.tool_use_id,
                  subagentType: event.subagent_type,
                  description: event.description,
                },
              ],
            },
          };
        }
        case 'subagent_finished': {
          // The subagent completed (foreground `PostToolUse(Agent)`). Drop it
          // by `tool_use_id`; when it was the session's last running subagent,
          // drop the now-empty entry so the indicator disappears. A finish for
          // an id not tracked (already swept at turn end) changes nothing.
          const current = state.runningSubagents[event.session_id];
          if (
            current === undefined ||
            !current.some((s) => s.toolUseId === event.tool_use_id)
          ) {
            return state;
          }
          const remaining = current.filter(
            (s) => s.toolUseId !== event.tool_use_id,
          );
          const runningSubagents = { ...state.runningSubagents };
          if (remaining.length === 0) {
            delete runningSubagents[event.session_id];
          } else {
            runningSubagents[event.session_id] = remaining;
          }
          return { runningSubagents };
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
            true,
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        default:
          return state;
      }
    }),
}));
