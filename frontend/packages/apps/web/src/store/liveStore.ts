import { create } from 'zustand';
import {
  loadPersistedStatus,
  savePersistedStatus,
} from './statusPersistence';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  PendingPermission,
  PendingQuestion,
  RunningSubagent,
  SessionEvent,
  StatusSnapshot,
  Turn,
} from '@delta/wire-gen';
import type { ConnectionStatus } from '@delta/api-client';
import type { RateLimits } from './statusTypes';

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
  /**
   * Set when a `turn_completed` / `turn_interrupted` for this submit's session
   * arrived BEFORE the POST's `onSuccess` ran — the load race
   * {@link LiveState.endTurnForSession} detects. The POST's caller (the submit
   * hook) reads this flag in `onSuccess`: if set, the send is dropped instead
   * of staged into {@link LiveState.localSends}, so a chip with no remaining
   * drain trigger never lands. A normal POST without a racing turn-end never
   * carries the flag, so the standard `recordLocalSend` path runs.
   *
   * Storing the race signal directly on the in-flight submit keeps the race
   * detection scoped to the same record that already represents "this POST is
   * mid-flight" — no separate per-session counter that has to be kept in step
   * with the {@link sending} array.
   */
  dropOnResolve?: true;
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
 * (correlated by {@link toolUseId}), and re-seeded from the sends envelope's
 * `running_subagents` after a reconnect, so a missed start/finish event heals
 * from a plain refetch.
 *
 * The {@link background} flag drives the turn-end sweep. A FOREGROUND subagent
 * is swept when the turn ends / the session closes (it cannot outlive its
 * turn). A BACKGROUND subagent (`run_in_background: true`) outlives the
 * launching turn — the immediate `subagent_finished` of the launch never
 * arrives, and its real completion (a `subagent_finished` driven by the
 * completion notification) lands much later — so the turn-end sweep KEEPS it.
 */
export interface SubagentActivity {
  /**
   * The thread that launched the subagent. A subagent — a BACKGROUND one in
   * particular — keeps its launching thread "running" until it finishes, even
   * past the end of the launching turn, so the navigator can keep that thread's
   * spinner lit and suppress its unread badge until the subagent is done (see
   * {@link threadHasRunningSubagent}). `subagent_finished` carries no thread;
   * this entry is what maps the finishing `tool_use_id` back to its thread.
   */
  threadId: ThreadId;
  /** The `Agent`/`Task` call's `tool_use_id` (its stable correlation key). */
  toolUseId: string;
  /** The subagent type (e.g. `general-purpose`), or null if none was given. */
  subagentType: string | null;
  /** The short task description for display, or null if none was given. */
  description: string | null;
  /**
   * Whether the launch carried `run_in_background: true`. A background subagent
   * survives the turn-end sweep; a foreground one is dropped at turn end.
   */
  background: boolean;
}

/**
 * Whether a thread is "running" in the navigator sense: it has either an
 * in-flight turn or a still-running subagent it launched. A BACKGROUND subagent
 * outlives its launching turn, so a thread that launched one is NOT idle while
 * it runs — folding the subagent into "running" keeps that thread's spinner lit
 * and, crucially, suppresses its `unread && !running` "done" badge until the
 * subagent finishes. Both inputs are the per-session slices
 * (`runningThreads[sessionId]` and `runningSubagents[sessionId]`).
 */
export function threadIsRunning(
  runningThreads: Record<ThreadId, true> | undefined,
  runningSubagents: SubagentActivity[] | undefined,
  threadId: ThreadId,
): boolean {
  if (runningThreads?.[threadId]) {
    return true;
  }
  return (runningSubagents ?? []).some((s) => s.threadId === threadId);
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
  /**
   * The in-flight turn's thread the question was asked on, mirroring
   * {@link ExternalInputNotice.threadId}: the transcript pane gates the
   * question card to this thread so it shows only where the question belongs,
   * not on every thread of the session.
   */
  threadId: ThreadId;
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
   * Threads with a turn in flight, keyed by session id then thread id, set by
   * `turn_started` and cleared when the turn completes or is interrupted (or
   * the session closes). Drives the navigator's THREAD-aware "running"
   * indicator: a session's collapsed row OR-aggregates over its inner record
   * (any running thread → spinner), while the expanded thread tree reads each
   * thread's own flag. Keyed by session first so a `session_closed` (which
   * carries only a session id) clears the whole session in one drop, with no
   * separate thread→session map to keep in step. Note `turn_started` only fires
   * when the user line was ingested in the same `UserPromptSubmit` sync, so
   * absence here does not prove idleness — same semantics the FIFO's
   * `in_progress` status had.
   */
  runningThreads: Record<SessionId, Record<ThreadId, true>>;
  /**
   * Per-session notices, at most one per {@link SessionNoticeKind} per
   * session. Read through {@link noticeOf}; lifecycle clearing follows
   * {@link NOTICE_LIFECYCLE}.
   */
  notices: Record<SessionId, SessionNotice[]>;
  /**
   * Unread counts keyed by thread id; cleared when a thread becomes active.
   *
   * The single source of truth for unread, at thread granularity. Bumped on a
   * `turn_completed` for a thread the user is not currently viewing (a
   * background session, or a non-active thread of the focused session) and on
   * external input to the focused thread. The navigator's collapsed session row
   * OR-aggregates over the session's threads (any unread → dot), and the
   * expanded thread tree shows each thread's own count — so there is no separate
   * session-level unread map to drift from this one. In-memory only (resets on
   * reload): persistence across reload would need backend support and is out of
   * scope.
   */
  unread: Record<ThreadId, number>;
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
   * The latest Claude Code status-line context-usage percentage per session,
   * keyed by session id. Replace-latest, not append: the status line fires
   * frequently, so only the most recent snapshot of each session is kept. Drives
   * the composer's top-edge context bar for the focused session. A session with
   * no snapshot yet (or a `null` percentage right after `/compact`) has no entry,
   * so the bar is hidden rather than shown at 0%.
   */
  contextUsage: Record<SessionId, number>;
  /**
   * The latest account-wide rate-limit snapshot, or `null` before any
   * `status_updated` event arrives. A single global value (not per session):
   * rate limits are account-wide, so every session's status line reports the
   * same windows and the most recent one wins. Drives the navigator footer's
   * 5h/7d meter rows; an absent window hides its row.
   */
  rateLimits: RateLimits | null;

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
   * Drop one tracked local send by id (a no-op if no entry exists). Used by
   * the cancel-send mutation: the server flips the row to `cancelled` and
   * drops it from the open list, but the turn-end events that normally drain
   * `localSends` never fire for a cancel — so the tracked twin would linger
   * as a stuck `local` chip with no per-row indicator. Dropping it here
   * keeps the strip in sync with the server.
   */
  forgetLocalSend: (sendId: number) => void;
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
   * running-thread flags, and the permission/question notices. Used on a
   * live-stream reconnect: the turn-end / `permission_resolved` events that
   * would have drained these were broadcast while the socket was down and are
   * not replayed, so they can no longer be reconciled from events. They all
   * recover from the refetched sends envelope — the open-send list by refetch,
   * the running-thread flag via {@link seedActiveTurn}, the permission notice via
   * {@link seedPermission}, and the question notice via {@link seedQuestion}.
   * Other notice kinds stay: they cannot be re-seeded, and each has a
   * non-event escape hatch (a user dismiss or a lifecycle trigger).
   */
  resetTurnEphemera: () => void;
  /**
   * Seed a session's running-thread flag from the server's queryable turn state
   * (the `turn` field of `GET /api/sessions/{id}/sends`), which carries the
   * in-flight turn's `thread_id` so the flag lands on the exact thread.
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
   *   server is the source of truth, so reconcile the session's running-thread
   *   set to match. A fresh `in_flight` keeps/re-sets the flag on `thread_id`
   *   (so reconnect healing still works when the resync refetch lands
   *   `in_flight`); a fresh `idle` (or `awaiting_echo`, or an `in_flight` whose
   *   `thread_id` is absent) authoritatively means "no running thread here" and
   *   CLEARS the whole session's set.
   *
   * The authoritative mode exists to clear a flag the stale-cache read would
   * otherwise resurrect: after a turn completes off-focus its `turn_completed`
   * clears the running thread, but re-focusing the session serves the stale
   * cached `in_flight` envelope before the refetch — without an authoritative
   * clear on the following fresh `idle`, the set-only re-seed would leave the
   * spinner stuck on. Callers must therefore pass `authoritative: true` only for
   * a read known to be fresh (the query settled, not a stale-cache placeholder
   * shown mid-refetch).
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
 * Drop the FOREGROUND running subagents of one session at turn end, KEEPING any
 * background entries, and return the changed slice (empty object when nothing
 * changed). A foreground subagent cannot outlive the turn that spawned it, so
 * it is swept; a background subagent (`run_in_background: true`) deliberately
 * outlives the launching turn and is removed only by its completion
 * `subagent_finished`, so it is kept.
 */
function dropForegroundSubagentsForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  const current = state.runningSubagents[sessionId];
  if (!current) {
    return {};
  }
  const survivors = current.filter((s) => s.background);
  if (survivors.length === current.length) {
    // All entries are background: nothing to sweep, keep identity-stable state.
    return {};
  }
  const runningSubagents = { ...state.runningSubagents };
  if (survivors.length === 0) {
    delete runningSubagents[sessionId];
  } else {
    runningSubagents[sessionId] = survivors;
  }
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
/**
 * Clear a running-thread flag. When `threadId` is given (a turn-end on a
 * specific thread) only that thread is cleared, dropping the session's record
 * once its last running thread goes; when it is `null` (a `session_closed`,
 * which ends every thread of the session) the whole session entry is dropped.
 * Returns the changed slice, or an empty object when nothing matched so the
 * caller can keep the identity-stable state.
 */
function clearRunningThread(
  state: LiveState,
  sessionId: SessionId,
  threadId: ThreadId | null,
): Partial<LiveState> {
  const current = state.runningThreads[sessionId];
  if (current === undefined) {
    return {};
  }
  const runningThreads = { ...state.runningThreads };
  if (threadId === null) {
    delete runningThreads[sessionId];
    return { runningThreads };
  }
  if (!current[threadId]) {
    return {};
  }
  const remaining = { ...current };
  delete remaining[threadId];
  if (Object.keys(remaining).length === 0) {
    delete runningThreads[sessionId];
  } else {
    runningThreads[sessionId] = remaining;
  }
  return { runningThreads };
}

/**
 * Mark the first in-flight submit that could be racing this turn-end as
 * "drop on POST resolve", returning the changed `sending` slice (empty object
 * when nothing matched). A turn-end carries only a session id, while a
 * new-session submit's target intentionally carries no session id — the POST
 * response mints it — so any in-flight new-session POST is a possible racer for
 * the ending session and qualifies just like a thread submit aimed at it. The
 * eligible item is the OLDEST submit on the `sending` array still in the
 * `sending` status without a flag yet, so two concurrent racing POSTs each get
 * flagged by their own turn-end in submit order.
 */
function flagRacedSendingForSession(
  state: LiveState,
  sessionId: SessionId,
): Partial<LiveState> {
  const idx = state.sending.findIndex(
    (item) =>
      item.status === 'sending' &&
      item.dropOnResolve === undefined &&
      ((item.target.kind === 'thread' &&
        item.target.sessionId === sessionId) ||
        item.target.kind === 'new-session'),
  );
  if (idx === -1) {
    return {};
  }
  const sending = state.sending.slice();
  sending[idx] = { ...sending[idx], dropOnResolve: true };
  return { sending };
}

function endTurnForSession(
  state: LiveState,
  sessionId: SessionId,
  trigger: 'turn_end' | 'session_closed',
  threadId: ThreadId | null,
  dropStreaming: boolean,
): Partial<LiveState> {
  const next: Partial<LiveState> = dropLocalSendsForSession(state, sessionId);
  // A turn ended but drained no tracked local send, yet a submit on this
  // session is still mid-POST: the turn-end raced ahead of that POST's
  // `onSuccess`. Flag the racing submit so its imminent `onSuccess` drops the
  // already-ended send instead of staging a chip with no future drain trigger.
  // Gated on an in-flight submit so a normal already-drained turn-end (or a
  // direct-pane turn with no browser submit) flags nothing. `session_closed`
  // is excluded: a closed session accepts no further sends, so no late
  // `recordLocalSend` can follow. The flag is always consumed and never leaks:
  // the server runs a turn only for an ACCEPTED send, so a turn-end implies
  // its POST returned 2xx, whose `onSuccess` is the very consumer of the flag.
  // A rejected POST has no turn, so it cannot have raced a turn-end here.
  const flagged =
    trigger === 'turn_end' && next.localSends === undefined
      ? flagRacedSendingForSession(state, sessionId)
      : {};
  return {
    ...next,
    ...flagged,
    ...clearRunningThread(state, sessionId, threadId),
    ...(dropStreaming ? dropStreamingForSession(state, sessionId) : {}),
    // A FOREGROUND subagent cannot outlive the turn that spawned it, so any
    // still-running foreground entry is cleared whenever the turn ends (or the
    // session closes); this also covers a foreground `subagent_finished` that
    // was missed. A BACKGROUND subagent (`run_in_background: true`) outlives the
    // launching turn and is KEPT — it is finished only by its completion
    // `subagent_finished`.
    ...dropForegroundSubagentsForSession(state, sessionId),
    ...clearNoticesOn(state.notices, sessionId, trigger),
  };
}

// Seed the status slices from the last persisted snapshot (freshness-guarded),
// so a reload restores the context bar / rate-limit footer instead of going
// blank until the next statusLine event.
const restoredStatus = loadPersistedStatus(Date.now());

export const useLiveStore = create<LiveState>((set) => ({
  connection: 'connecting',
  sending: [],
  localSends: {},
  spawns: [],
  runningThreads: {},
  notices: {},
  unread: {},
  streamingMessages: {},
  runningSubagents: {},
  contextUsage: restoredStatus.contextUsage,
  rateLimits: restoredStatus.rateLimits,

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

  forgetLocalSend: (sendId) =>
    set((state) => {
      if (!(sendId in state.localSends)) {
        return state;
      }
      const rest = { ...state.localSends };
      delete rest[sendId];
      return { localSends: rest };
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
      // A `dropOnResolve` flag on an in-flight submit was set by the turn-end
      // events that may now be missing across the outage; without the turn-end
      // we cannot prove the send's turn is over, so clear the flags and let
      // the POST `onSuccess` stage the send normally (the refetched sends list
      // is the remaining truth for what the server still considers open).
      const sending = state.sending.map((item) => {
        if (item.dropOnResolve !== true) {
          return item;
        }
        const copy = { ...item };
        delete copy.dropOnResolve;
        return copy;
      });
      // The live previews' turn-end clears may also have been missed during the
      // outage and cannot be recovered (no re-seed of a partial stream this
      // PR), so drop them too — the flushed message renders from the refetch.
      return {
        sending,
        localSends: {},
        runningThreads: {},
        notices,
        streamingMessages: {},
        // The running-subagent set is re-seeded authoritatively from the sends
        // envelope's `running_subagents` on the resync refetch (see
        // {@link seedRunningSubagents}), so drop the event-reconstructed copy —
        // a `subagent_started`/`subagent_finished` missed during the outage is
        // not replayed.
        runningSubagents: {},
      };
    }),

  seedActiveTurn: (sessionId, turn, authoritative) =>
    set((state) => {
      // A turn is "running" for seeding only when it is in flight AND the
      // envelope resolved its thread (the `in_progress_turn_thread` result).
      // An `in_flight` with no thread cannot be placed on a thread, so it is
      // treated as not-running here.
      const runningThreadId =
        turn.state === 'in_flight' && turn.thread_id !== null
          ? (turn.thread_id as ThreadId)
          : null;
      const current = state.runningThreads[sessionId];
      if (!authoritative) {
        // Possibly-stale read: set-only healing. Never clear from here —
        // turn-end events own clearing, so a stale `idle` cannot wipe a flag
        // a live event just set.
        if (runningThreadId === null || current?.[runningThreadId]) {
          return state;
        }
        return {
          runningThreads: {
            ...state.runningThreads,
            [sessionId]: { ...current, [runningThreadId]: true },
          },
        };
      }
      // Fresh read: the server is authoritative, so reconcile the session's set
      // to exactly the running thread it reports (or empty when none).
      if (runningThreadId === null) {
        if (current === undefined) {
          return state;
        }
        const runningThreads = { ...state.runningThreads };
        delete runningThreads[sessionId];
        return { runningThreads };
      }
      // Already exactly this one running thread: keep identity-stable state.
      if (
        current !== undefined &&
        Object.keys(current).length === 1 &&
        current[runningThreadId]
      ) {
        return state;
      }
      return {
        runningThreads: {
          ...state.runningThreads,
          [sessionId]: { [runningThreadId]: true },
        },
      };
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
          threadId: question.thread_id,
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
          threadId: s.thread_id as ThreadId,
          toolUseId: s.tool_use_id,
          subagentType: s.subagent_type,
          description: s.description,
          background: s.background,
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
        case 'turn_started': {
          // The send correlated with its transcript line and the turn is
          // confirmed in flight. The chip itself follows the send (server
          // list + localSends); here only the per-thread running flag moves —
          // set on the exact thread the dispatched send took its turn on, so
          // the navigator lights the spinner on that thread (and OR-aggregates
          // it onto the collapsed session row).
          const current = state.runningThreads[event.session_id];
          if (current?.[event.thread_id]) {
            return state;
          }
          return {
            runningThreads: {
              ...state.runningThreads,
              [event.session_id]: { ...current, [event.thread_id]: true },
            },
          };
        }
        case 'turn_completed': {
          // The turn ended: clear the running flag on the exact thread that ran,
          // drain the session's tracked local sends — the server's open-send
          // list (refetched by the router) is the remaining truth for anything
          // still queued — and sweep the turn-scoped notices (see
          // NOTICE_LIFECYCLE). Scoped by session so a turn in one session never
          // drains another session's chips. The streaming preview is left in
          // place (dropStreaming: false): the persisted message will suppress
          // the bubble when it lands, a gap-free swap (see endTurnForSession).
          const next = endTurnForSession(
            state,
            event.session_id,
            'turn_end',
            event.thread_id,
            false,
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'turn_interrupted': {
          // The user interrupted the in-flight turn (Escape / Ctrl-C).
          // Claude's `Stop` hook does not fire on interrupt, so
          // `turn_completed` never arrives; the backend detects the interrupt
          // from the transcript and emits this hook-independent signal. Drain
          // exactly as a completed turn would (clearing the same thread's
          // running flag), but also drop the streaming preview
          // (dropStreaming: true): an interrupted partial may have no matching
          // persisted message, so nothing else would clear it.
          const next = endTurnForSession(
            state,
            event.session_id,
            'turn_end',
            event.thread_id,
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
              threadId: event.thread_id,
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
                  threadId: event.thread_id as ThreadId,
                  toolUseId: event.tool_use_id,
                  subagentType: event.subagent_type,
                  description: event.description,
                  background: event.background,
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
            null,
            true,
          );
          return Object.keys(next).length > 0 ? next : state;
        }
        case 'status_updated': {
          // A Claude Code status-line snapshot arrived. These fire frequently,
          // so both pieces are stored replace-latest (never appended): the
          // session's context-usage percentage replaces that session's previous
          // value, and the account-wide rate limits replace the single global
          // snapshot (every session reports the same windows, so the most recent
          // event wins regardless of which session it came from).
          const snapshot: StatusSnapshot = event.snapshot;
          const next: Partial<LiveState> = {};

          // Context usage is per session. A `null` percentage (e.g. right after
          // `/compact`, before the next API response) drops the session's entry
          // so the bar is hidden rather than pinned at the old value or 0%.
          const pct = snapshot.context_used_percentage;
          if (pct === null) {
            if (state.contextUsage[event.session_id] !== undefined) {
              const contextUsage = { ...state.contextUsage };
              delete contextUsage[event.session_id];
              next.contextUsage = contextUsage;
            }
          } else if (state.contextUsage[event.session_id] !== pct) {
            next.contextUsage = {
              ...state.contextUsage,
              [event.session_id]: pct,
            };
          }

          // Rate limits are account-wide: replace the single global snapshot.
          next.rateLimits = {
            fiveHour: snapshot.five_hour,
            sevenDay: snapshot.seven_day,
          };

          // Persist the latest snapshot so a reload can restore it (freshness-
          // guarded in statusPersistence) instead of going blank.
          savePersistedStatus(
            next.contextUsage ?? state.contextUsage,
            next.rateLimits,
            Date.now(),
          );

          return next;
        }
        default:
          return state;
      }
    }),
}));
