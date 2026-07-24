import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { EventReducer } from './eventReducer';
import type { NoticesSlice, SessionNotice } from './noticesSlice';
import { clearNoticesOn } from './noticesSlice';
import type { SendsSlice } from './sendsSlice';
import {
  dropLocalSendsForSession,
  flagRacedSendingForSession,
} from './sendsSlice';
import type { RunningThreadsSlice } from './runningThreadsSlice';
import { clearRunningThread } from './runningThreadsSlice';
import type { StreamingSlice } from './streamingSlice';
import { dropStreamingForSession } from './streamingSlice';
import type { SubagentsSlice } from './subagentsSlice';
import { dropForegroundSubagentsForSession } from './subagentsSlice';

/**
 * Every state family a turn's end (or a reconnect reset) can touch: the
 * cross-family working set {@link endTurnForSession} sweeps and
 * {@link TurnLifecycleSlice.resetTurnEphemera} rebuilds from scratch.
 */
type TurnEphemeraState = Pick<SendsSlice, 'sending' | 'localSends'> &
  Pick<RunningThreadsSlice, 'runningThreads'> &
  Pick<StreamingSlice, 'streamingMessages'> &
  Pick<SubagentsSlice, 'runningSubagents'> &
  Pick<NoticesSlice, 'notices'>;

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
export function endTurnForSession(
  state: TurnEphemeraState,
  sessionId: SessionId,
  trigger: 'turn_end' | 'session_closed',
  threadId: ThreadId | null,
  dropStreaming: boolean,
): Partial<TurnEphemeraState> {
  const next: Partial<TurnEphemeraState> = dropLocalSendsForSession(
    state,
    sessionId,
  );
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

export interface TurnLifecycleSlice {
  /**
   * Drop the event-reconstructed turn-scoped state: tracked local sends, the
   * running-thread flags, and the permission/question notices. Used on a
   * live-stream reconnect: the turn-end / `permission_resolved` events that
   * would have drained these were broadcast while the socket was down and are
   * not replayed, so they can no longer be reconciled from events. They all
   * recover from the refetched sends envelope — the open-send list by refetch,
   * the running-thread flag via {@link RunningThreadsSlice.seedActiveTurn}, the
   * permission notice via {@link NoticesSlice.seedPermission}, and the question
   * notice via {@link NoticesSlice.seedQuestion}. Other notice kinds stay: they
   * cannot be re-seeded, and each has a non-event escape hatch (a user dismiss
   * or a lifecycle trigger).
   */
  resetTurnEphemera: () => void;
}

export const createTurnLifecycleSlice: StateCreator<
  TurnEphemeraState & TurnLifecycleSlice,
  [],
  [],
  TurnLifecycleSlice
> = (set) => ({
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
});

// The turn ended: clear the running flag on the exact thread that ran,
// drain the session's tracked local sends — the server's open-send
// list (refetched by the router) is the remaining truth for anything
// still queued — and sweep the turn-scoped notices (see
// NOTICE_LIFECYCLE). Scoped by session so a turn in one session never
// drains another session's chips. The streaming preview is left in
// place (dropStreaming: false): the persisted message will suppress
// the bubble when it lands, a gap-free swap (see endTurnForSession).
export const reduceTurnCompleted: EventReducer<
  TurnEphemeraState,
  'turn_completed'
> = (state, event) => {
  const next = endTurnForSession(
    state,
    event.session_id,
    'turn_end',
    event.thread_id,
    false,
  );
  return Object.keys(next).length > 0 ? next : state;
};

// The user interrupted the in-flight turn (Escape / Ctrl-C).
// Claude's `Stop` hook does not fire on interrupt, so
// `turn_completed` never arrives; the backend detects the interrupt
// from the transcript and emits this hook-independent signal. Drain
// exactly as a completed turn would (clearing the same thread's
// running flag), but also drop the streaming preview
// (dropStreaming: true): an interrupted partial may have no matching
// persisted message, so nothing else would clear it.
export const reduceTurnInterrupted: EventReducer<
  TurnEphemeraState,
  'turn_interrupted'
> = (state, event) => {
  const next = endTurnForSession(
    state,
    event.session_id,
    'turn_end',
    event.thread_id,
    true,
  );
  return Object.keys(next).length > 0 ? next : state;
};

// Closed state itself is reflected by the sessions query. But a
// closed session has no live process, so its turn (if any) is over
// and its turn-scoped notices are moot — drain exactly as a turn
// end would.
export const reduceSessionClosed: EventReducer<
  TurnEphemeraState,
  'session_closed'
> = (state, event) => {
  const next = endTurnForSession(
    state,
    event.session_id,
    'session_closed',
    null,
    true,
  );
  return Object.keys(next).length > 0 ? next : state;
};
