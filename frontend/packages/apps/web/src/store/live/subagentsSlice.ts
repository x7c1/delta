import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { RunningSubagent } from '@delta/wire-gen';
import type { EventReducer } from './eventReducer';

/** The running-subagent state alone, the only field this module touches. */
type SubagentsState = Pick<SubagentsSlice, 'runningSubagents'>;

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

export interface SubagentsSlice {
  /**
   * The subagents currently running in each session's turn, keyed by session id
   * and kept in start order. Added by `subagent_started`, removed by the
   * matching `subagent_finished`, swept on turn end / close, and re-seeded from
   * the sends envelope after a reconnect (see {@link SubagentActivity}). A
   * session with none running has no entry (the empty list is dropped).
   */
  runningSubagents: Record<SessionId, SubagentActivity[]>;

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
}

/**
 * Drop the FOREGROUND running subagents of one session at turn end, KEEPING any
 * background entries, and return the changed slice (empty object when nothing
 * changed). A foreground subagent cannot outlive the turn that spawned it, so
 * it is swept; a background subagent (`run_in_background: true`) deliberately
 * outlives the launching turn and is removed only by its completion
 * `subagent_finished`, so it is kept.
 */
export function dropForegroundSubagentsForSession(
  state: SubagentsState,
  sessionId: SessionId,
): Partial<SubagentsState> {
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

export const createSubagentsSlice: StateCreator<
  SubagentsSlice,
  [],
  [],
  SubagentsSlice
> = (set) => ({
  runningSubagents: {},

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
});

// A subagent (the `Agent`/`Task` tool) started in the main turn. It
// runs in its own (untailed) transcript, so this is the only live
// signal — add it to the session's running set so the navigator badge
// and conversation indicator appear. Keyed by `tool_use_id`: a
// duplicate start for an id already tracked changes nothing (a retried
// event), and new entries append so the set stays in start order.
export const reduceSubagentStarted: EventReducer<
  SubagentsState,
  'subagent_started'
> = (state, event) => {
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
};

// The subagent completed (foreground `PostToolUse(Agent)`). Drop it
// by `tool_use_id`; when it was the session's last running subagent,
// drop the now-empty entry so the indicator disappears. A finish for
// an id not tracked (already swept at turn end) changes nothing.
export const reduceSubagentFinished: EventReducer<
  SubagentsState,
  'subagent_finished'
> = (state, event) => {
  const current = state.runningSubagents[event.session_id];
  if (
    current === undefined ||
    !current.some((s) => s.toolUseId === event.tool_use_id)
  ) {
    return state;
  }
  const remaining = current.filter((s) => s.toolUseId !== event.tool_use_id);
  const runningSubagents = { ...state.runningSubagents };
  if (remaining.length === 0) {
    delete runningSubagents[event.session_id];
  } else {
    runningSubagents[event.session_id] = remaining;
  }
  return { runningSubagents };
};
