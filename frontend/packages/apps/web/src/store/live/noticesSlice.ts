import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type {
  FileChangeDetail,
  PendingPermission,
  PendingQuestion,
} from '@delta/wire-gen';
import type { EventReducer } from './eventReducer';
import type { SendsSlice } from './sendsSlice';

/** The notices state alone, the only fields this module's reducers touch. */
type NoticesState = Pick<NoticesSlice, 'notices'>;

/** One pending permission request, as the notice's queue holds it. */
export interface QueuedPermissionRequest {
  requestId: number;
  toolName: string;
  /** The tool input, serialized as JSON text (shown summarized). */
  toolInput: string;
  /**
   * What allowing the request would do to files on disk, when the provider
   * stated it: the affected paths, how each changes, the diffs, and the
   * provider's reason. `undefined` whenever nothing is known — every request
   * that is not a file change, and a file change whose detail the server could
   * not correlate — and the card then falls back to summarizing
   * {@link toolInput}.
   */
  fileChange?: FileChangeDetail;
  /**
   * A directory the request also asks to be allowed to write under for the rest
   * of the session, when the provider asked for one. `undefined` when it asked
   * for no such root.
   *
   * Independent of {@link fileChange} and broader than it: a request can carry
   * this with no change set at all, which is exactly the case where the card
   * would otherwise show only the input summary.
   */
  grantRoot?: string;
}

/**
 * The pending permission prompts blocking a session until they are answered — in
 * the browser (the notice's Allow/Deny) or in the terminal. The focused
 * session's notice drives the floating card over the transcript, and any
 * session's drives a badge on its navigator row.
 *
 * The card shows ONE request — this entry's own {@link requestId} — because a
 * decision is one answer to one question. But several can be outstanding at
 * once: a provider that runs tool calls in parallel raises N approvals in the
 * same instant. Those queue in {@link queued} behind the shown one, oldest
 * first, so N rapid `permission_requested` events leave the FIRST request on
 * screen (not the last) and answering it promotes the next.
 *
 * Set by `permission_requested` and re-seeded from the sends envelope's
 * `permission` / `permission_count` after a reconnect (events are not
 * replayed). `permission_resolved` for the shown request promotes the next
 * queued one (or removes the notice when none is left); for a queued one it
 * removes just that entry. The whole notice also goes when the turn ends and
 * when the session closes. A user dismiss only flags the entry
 * {@link dismissed} — removing it would let the next refetch re-seed the same
 * still-pending request and resurrect the card the user just closed.
 */
export interface PermissionNotice {
  kind: 'permission';
  requestId: number;
  toolName: string;
  /** The tool input, serialized as JSON text (shown summarized). */
  toolInput: string;
  /** See {@link QueuedPermissionRequest.fileChange}. */
  fileChange?: FileChangeDetail;
  /** See {@link QueuedPermissionRequest.grantRoot}. */
  grantRoot?: string;
  /** True once the user dismissed the card; the entry stays for de-dup. */
  dismissed: boolean;
  /**
   * The requests waiting behind the shown one, oldest first. Empty in the
   * ordinary single-dialog case (and always empty for a pane-backed provider,
   * whose hook blocks until each dialog is answered).
   */
  queued: QueuedPermissionRequest[];
  /**
   * How many requests the session has pending in total, the shown one included
   * — so at least `1 + queued.length`. It can exceed that: after a reconnect the
   * envelope reports the head plus a depth, and the identities of the requests
   * behind it are only learned from later events. This is what the card's
   * remaining-count indication reads.
   */
  pendingCount: number;
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
 * A composed message was never delivered: two dispatches in a row produced no
 * matching echo, so the server parked it (cancelled the row) rather than
 * re-typing it on every idle forever. The notice is what keeps that from being
 * a silent loss — it shows the text back to the user so they can copy or
 * re-send it.
 *
 * Session-scoped, not thread-scoped: an undelivered message matters wherever
 * the user is looking. Survives turn ends (see the lifecycle table below);
 * removed on dismiss or when the session closes.
 */
export interface SendParkedNotice {
  kind: 'send_parked';
  sendId: number;
  /** The composed message that never reached the session. */
  text: string;
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
 * A `spawn_failed` that arrived before {@link SpawnsSlice.trackSpawn} registered
 * the spawn. The event is broadcast on the live channel while the
 * `POST /api/sends` response travels back separately, so the failure can
 * legitimately outrun the registration; dropping it would leave the chip
 * spinning forever. Never rendered: buffered here and consumed by
 * {@link SpawnsSlice.trackSpawn}, which then registers the spawn as `failed`
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
  | SendParkedNotice
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
export type NoticeClearTrigger =
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
  // An undelivered message stays undelivered when the turn ends — and the park
  // happens inside a turn that is about to end, so a `turn_end` sweep would
  // erase the notice seconds after it appeared.
  send_parked: ['session_closed'],
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
export function withNotice(
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
export function removeNotices(
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
export function clearNoticesOn(
  notices: Record<SessionId, SessionNotice[]>,
  sessionId: SessionId,
  trigger: NoticeClearTrigger,
): { notices: Record<SessionId, SessionNotice[]> } | Record<string, never> {
  return removeNotices(notices, sessionId, (notice) =>
    NOTICE_LIFECYCLE[notice.kind].includes(trigger),
  );
}

export interface NoticesSlice {
  /**
   * Per-session notices, at most one per {@link SessionNoticeKind} per
   * session. Read through {@link noticeOf}; lifecycle clearing follows
   * {@link NOTICE_LIFECYCLE}.
   */
  notices: Record<SessionId, SessionNotice[]>;

  /** Flag a session as resume-impossible, surfacing the inline notice. */
  markResumeUnavailable: (sessionId: SessionId) => void;
  /** Clear a session's resume-impossible flag (e.g. once it opens). */
  clearResumeUnavailable: (sessionId: SessionId) => void;
  /**
   * Seed a session's permission notice from the server's queryable pending
   * queue (the `permission` head and `permission_count` depth of
   * `GET /api/sessions/{id}/sends`).
   * Mirrors {@link RunningThreadsSlice.seedActiveTurn}: set-only (`null`
   * clears nothing — clearing is owned by the events and the lifecycle
   * sweeps), and a report of the request the notice already shows leaves the
   * card alone, so a refetch can neither resurrect a notice an event just
   * resolved nor un-dismiss one the user closed — only its remaining count
   * catches up.
   */
  seedPermission: (
    sessionId: SessionId,
    permission: PendingPermission | null,
    pendingCount: number,
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
  /** Dismiss the parked-send notice for a session. */
  dismissSendParked: (sessionId: SessionId) => void;
}

export const createNoticesSlice: StateCreator<
  NoticesSlice,
  [],
  [],
  NoticesSlice
> = (set) => ({
  notices: {},

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

  seedPermission: (sessionId, permission, pendingCount) =>
    set((state) => {
      if (permission === null) {
        return state;
      }
      const current = noticeOf(state.notices, sessionId, 'permission');
      if (current?.requestId === permission.request_id) {
        // Same head: keep the card (and its dismissed flag) exactly as it is,
        // and only let the server's depth correct the remaining count — a client
        // that missed the events for the queued requests knows their number from
        // here even though it knows none of their identities. Floored at what
        // this client can already see, since the snapshot may predate events it
        // has applied. (The event reducers need no such floor: their arithmetic
        // moves the count and the queue together.)
        const depth = Math.max(pendingCount, 1 + current.queued.length);
        if (depth === current.pendingCount) {
          return state;
        }
        return {
          notices: withNotice(state.notices, sessionId, {
            ...current,
            pendingCount: depth,
          }),
        };
      }
      // A different head. When the server's head is one of the requests this
      // client has queued, its own head must have resolved during the gap: drop
      // everything ahead of the reported head and keep the rest of the queue.
      // Otherwise the reported head is unknown here, and the envelope is the only
      // truth available — take it alone, with the server's depth.
      const promotedIndex = (current?.queued ?? []).findIndex(
        (request) => request.requestId === permission.request_id,
      );
      const queued =
        current && promotedIndex >= 0
          ? current.queued.slice(promotedIndex + 1)
          : [];
      return {
        notices: withNotice(state.notices, sessionId, {
          kind: 'permission',
          requestId: permission.request_id,
          toolName: permission.tool_name,
          toolInput: permission.tool_input,
          fileChange: permission.file_change,
          grantRoot: permission.grant_root,
          dismissed: false,
          queued,
          pendingCount: Math.max(pendingCount, 1 + queued.length),
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

  dismissSendParked: (sessionId) =>
    set((state) => {
      const next = removeNotices(
        state.notices,
        sessionId,
        (notice) => notice.kind === 'send_parked',
      );
      return Object.keys(next).length > 0 ? next : state;
    }),
});

// A tool is asking to proceed. The FIRST unanswered request owns the card: a
// provider running tool calls in parallel raises several at once, and swapping
// the card out from under the user (the last writer winning) is what left the
// other requests unanswerable. So a request arriving while one is shown queues
// behind it and only bumps the remaining count.
export const reducePermissionRequested: EventReducer<
  NoticesState,
  'permission_requested'
> = (state, event) => {
  const request: QueuedPermissionRequest = {
    requestId: event.request_id,
    toolName: event.tool_name,
    toolInput: event.tool_input,
    fileChange: event.file_change,
    grantRoot: event.grant_root,
  };
  const current = noticeOf(state.notices, event.session_id, 'permission');
  if (current === null) {
    return {
      notices: withNotice(state.notices, event.session_id, {
        kind: 'permission',
        ...request,
        dismissed: false,
        queued: [],
        pendingCount: 1,
      }),
    };
  }
  // Already shown, or already queued: a re-broadcast of the same request (the
  // server re-raises a promoted head, and a retried hook can repeat one) must
  // not queue it twice or reset its position.
  if (
    current.requestId === request.requestId ||
    current.queued.some((queued) => queued.requestId === request.requestId)
  ) {
    return state;
  }
  const queued = [...current.queued, request];
  return {
    notices: withNotice(state.notices, event.session_id, {
      ...current,
      queued,
      pendingCount: current.pendingCount + 1,
    }),
  };
};

// Claude Code's AskUserQuestion is presenting its options in the
// TUI; surface the dedicated question card. Driven off PreToolUse so
// the same `permission_resolved` (from the correlated tool_result)
// clears it once the user answers.
export const reduceQuestionAsked: EventReducer<
  NoticesState,
  'question_asked'
> = (state, event) => ({
  notices: withNotice(state.notices, event.session_id, {
    kind: 'question',
    requestId: event.request_id,
    threadId: event.thread_id,
    toolInput: event.tool_input,
    dismissed: false,
  }),
});

// The request was answered (a browser decision or the correlated
// tool_result). Only the resolved request leaves, so a stale resolution never
// wipes a newer pending prompt for the same session:
//
// - the SHOWN request → the next queued one takes the card (un-dismissed: it is
//   a question the user has not answered yet), or the notice goes when the queue
//   is empty. The server also re-broadcasts the promoted head, which this
//   client's own promotion makes a no-op — so the dialog survives either way;
// - a QUEUED request → that entry alone is dropped and the card stays put;
// - an unknown request → nothing changes.
//
// An auto-approved tool resolves almost immediately, so this clears the brief
// notice; a genuine prompt has no resolution until the human answers. The same
// event also clears a `question` notice with the matching request id: an
// AskUserQuestion's request row resolves the moment its tool_result (the user's
// pick) is ingested.
export const reducePermissionResolved: EventReducer<
  NoticesState,
  'permission_resolved'
> = (state, event) => {
  const permission = noticeOf(state.notices, event.session_id, 'permission');
  if (permission !== null) {
    if (permission.requestId === event.request_id) {
      const [promoted, ...rest] = permission.queued;
      // With no known successor the notice goes, even if the server reported a
      // deeper queue (the reconnect seed knows the depth but not the identities):
      // there is nothing to show a card for. The server's promotion broadcast
      // brings the next request in a moment, and the next envelope refetch
      // restores the true remaining count.
      if (promoted !== undefined) {
        return {
          notices: withNotice(state.notices, event.session_id, {
            kind: 'permission',
            ...promoted,
            dismissed: false,
            queued: rest,
            pendingCount: permission.pendingCount - 1,
          }),
        };
      }
    } else if (
      permission.queued.some(
        (queued) => queued.requestId === event.request_id,
      )
    ) {
      const queued = permission.queued.filter(
        (request) => request.requestId !== event.request_id,
      );
      return {
        notices: withNotice(state.notices, event.session_id, {
          ...permission,
          queued,
          pendingCount: permission.pendingCount - 1,
        }),
      };
    }
  }
  const next = removeNotices(
    state.notices,
    event.session_id,
    (notice) =>
      (notice.kind === 'permission' || notice.kind === 'question') &&
      notice.requestId === event.request_id,
  );
  return Object.keys(next).length > 0 ? next : state;
};

// Open/closed lifecycle is reflected by the sessions query, and the
// tracked spawn is cleared by the workspace once it can focus the
// freshly-listed session (it needs the id until then). The notice
// sweep drops a stale buffered spawn failure (see NOTICE_LIFECYCLE).
export const reduceSessionRegistered: EventReducer<
  NoticesState,
  'session_registered'
> = (state, event) => {
  const next = clearNoticesOn(
    state.notices,
    event.session_id,
    'session_registered',
  );
  return Object.keys(next).length > 0 ? next : state;
};

// The session resumed successfully; the sweep drops a stale "cannot
// be resumed" notice. Open/closed itself is reflected by the
// sessions query, not ephemeral here.
export const reduceSessionOpened: EventReducer<
  NoticesState,
  'session_opened'
> = (state, event) => {
  const next = clearNoticesOn(
    state.notices,
    event.session_id,
    'session_opened',
  );
  return Object.keys(next).length > 0 ? next : state;
};

// The external-input notice is session-scoped and only meaningful
// for the focused session, so the router (`applySessionEvent`)
// records it via `noteExternalInput` under a focus guard. Nothing
// to do here.
export const reduceExternalInput: EventReducer<
  NoticesState,
  'external_input'
> = (state) => state;

// A send the server gave up delivering. Unlike external input this is
// recorded for EVERY session, focused or not: the user's own message
// was dropped, and they must find out when they come back to that
// session, not only if they happened to be watching it.
//
// The park is also terminal for the send itself, so its tracked local twin is
// dropped here — the same reconciliation the cancel mutation does with
// `forgetLocalSend`. Without it the twin would render as an in-progress chip
// forever: the server row is gone from the open list (so nothing overrides the
// twin), and a park needs no turn to end — the echo-deadline watchdog parks a
// send whose turn NEVER started — so no turn-end sweep would ever drain it.
export const reduceSendParked: EventReducer<
  NoticesState & Pick<SendsSlice, 'localSends'>,
  'send_parked'
> = (state, event) => {
  const notices = withNotice(state.notices, event.session_id, {
    kind: 'send_parked',
    sendId: event.send_id,
    text: event.text,
    at: Date.now(),
  });
  if (!(event.send_id in state.localSends)) {
    return { notices };
  }
  const localSends = { ...state.localSends };
  delete localSends[event.send_id];
  return { notices, localSends };
};
