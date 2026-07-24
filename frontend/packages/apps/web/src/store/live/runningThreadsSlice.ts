import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { Turn } from '@delta/wire-gen';
import type { EventReducer } from './eventReducer';
import type { SubagentActivity } from './subagentsSlice';

/** The running-thread state alone, the only field this module touches. */
type RunningThreadsState = Pick<RunningThreadsSlice, 'runningThreads'>;

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

export interface RunningThreadsSlice {
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
}

/**
 * Clear a running-thread flag. When `threadId` is given (a turn-end on a
 * specific thread) only that thread is cleared, dropping the session's record
 * once its last running thread goes; when it is `null` (a `session_closed`,
 * which ends every thread of the session) the whole session entry is dropped.
 * Returns the changed slice, or an empty object when nothing matched so the
 * caller can keep the identity-stable state.
 */
export function clearRunningThread(
  state: RunningThreadsState,
  sessionId: SessionId,
  threadId: ThreadId | null,
): Partial<RunningThreadsState> {
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

export const createRunningThreadsSlice: StateCreator<
  RunningThreadsSlice,
  [],
  [],
  RunningThreadsSlice
> = (set) => ({
  runningThreads: {},

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
});

// The send correlated with its transcript line and the turn is
// confirmed in flight. The chip itself follows the send (server
// list + localSends); here only the per-thread running flag moves —
// set on the exact thread the dispatched send took its turn on, so
// the navigator lights the spinner on that thread (and OR-aggregates
// it onto the collapsed session row).
export const reduceTurnStarted: EventReducer<
  RunningThreadsState,
  'turn_started'
> = (state, event) => {
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
};
