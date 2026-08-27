import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { EventReducer } from './eventReducer';
import type { NoticesSlice } from './noticesSlice';
import { noticeOf, removeNotices, withNotice } from './noticesSlice';
import type { NewSessionLaunch, SendsSlice } from './sendsSlice';
import { dropLocalSendsForSession } from './sendsSlice';

/**
 * The state this module's action and reducer read: its own spawn registry,
 * plus the notices map (the buffered early failure lives there) and the
 * tracked local sends (a failed spawn's send is dropped — its turn never ends).
 */
type SpawnsState = Pick<SpawnsSlice, 'spawns'> &
  Pick<NoticesSlice, 'notices'> &
  Pick<SendsSlice, 'localSends'>;

/** A new-session spawn tracked from the POST response (real ids). */
export interface SpawnItem extends NewSessionLaunch {
  sessionId: SessionId;
  /** The spawned session's `main` thread (from the POST response). */
  threadId: ThreadId;
  /**
   * The first prompt, retained — with the launch configuration this extends —
   * so a failed spawn can be retried as the identical launch.
   */
  text: string;
  /** spawning: launch in flight; failed: reaped (`spawn_failed` arrived). */
  status: 'spawning' | 'failed';
  /**
   * Why the launch failed, when the `spawn_failed` named a cause — a git or
   * tmux message from the background launch preparation. Shown under the
   * failed chip's "failed to start" line, and the only place that message
   * appears: the send was accepted long before the failure, so no error
   * response could carry it. `undefined` while spawning, and for the
   * watchdog-shaped failures (a launch that exited, a spawn that never bound),
   * which observe only silence.
   */
  reason?: string;
}

export interface SpawnsSlice {
  /** Tracked new-session spawns, oldest first, keyed by real session id. */
  spawns: SpawnItem[];

  /**
   * Track a new-session spawn (real ids from the POST response). If the
   * spawn's failure already arrived (see {@link SpawnFailureBufferedNotice}),
   * the spawn is registered as `failed` immediately.
   */
  trackSpawn: (spawn: Omit<SpawnItem, 'status'>) => void;
  /**
   * Drop a tracked spawn. A spawn that comes up is released by its
   * `session_registered` event (see {@link reduceSessionRegistered}), so this
   * is the manual path: a failed spawn dismissed, or retried.
   */
  clearSpawn: (sessionId: SessionId) => void;
}

export const createSpawnsSlice: StateCreator<
  SpawnsState & SpawnsSlice,
  [],
  [],
  SpawnsSlice
> = (set) => ({
  spawns: [],

  trackSpawn: (spawn) =>
    set((state) => {
      const buffered = noticeOf(
        state.notices,
        spawn.sessionId,
        'spawn_failure_buffered',
      );
      if (!buffered) {
        return { spawns: [...state.spawns, { ...spawn, status: 'spawning' }] };
      }
      // The failure outran the POST response: register the spawn already
      // failed (the Retry/Dismiss chip surfaces right away, with the reason the
      // buffered failure carried), consume the buffered failure, and drop the
      // just-recorded local send for it — its turn will never end.
      return {
        spawns: [
          ...state.spawns,
          { ...spawn, status: 'failed', reason: buffered.reason },
        ],
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
});

// The launch preparation failed, or the spawn never bound and the server
// reaped it (the row is gone either way).
// Flip the tracked spawn to `failed` so the recoverable chip with
// Retry / Dismiss surfaces, and drop any tracked local send for it —
// its turn will never end. The event carries the REAL session id the
// POST response returned, so this is an exact match. An id with no
// tracked spawn at all is buffered, NOT dropped: the broadcast can
// outrun this client's own POST response, in which case `trackSpawn`
// consumes the buffer moments later (a genuinely foreign id — e.g.
// another client's spawn — leaves an inert entry).
export const reduceSpawnFailed: EventReducer<SpawnsState, 'spawn_failed'> = (
  state,
  event,
) => {
  const idx = state.spawns.findIndex(
    (spawn) =>
      spawn.sessionId === event.session_id && spawn.status === 'spawning',
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
            reason: event.reason,
          }),
        };
  }
  const spawns = state.spawns.slice();
  spawns[idx] = { ...spawns[idx], status: 'failed', reason: event.reason };
  return {
    spawns,
    ...dropLocalSendsForSession(state, event.session_id),
  };
};

/**
 * The spawn came up: its launch bound and the server activated the row. The
 * tracked entry has done its job — the workspace focused the session when the
 * POST accepted it, and the pending chip now renders from the session's own
 * open-send list — so drop it here.
 *
 * This is the release point precisely because it is the LAST thing the entry
 * is needed for: while a spawn is tracked the workspace refuses to reconcile
 * focus away from its id (the row may not be in the loaded page yet), and
 * `usePendingSends` shows its first prompt on the new-session surface for a
 * user who navigated back there. Only a `spawning` entry is dropped: a
 * `failed` one is a Retry/Dismiss card the user still has to answer, and a
 * registration for a session this client never spawned matches nothing.
 */
export const reduceSessionRegistered: EventReducer<
  SpawnsState,
  'session_registered'
> = (state, event) => {
  const spawns = state.spawns.filter(
    (spawn) =>
      !(spawn.sessionId === event.session_id && spawn.status === 'spawning'),
  );
  // Nothing matched — a foreign id, or an entry already flipped to `failed`.
  // Hand back the identity-stable state so subscribers are not notified.
  return spawns.length === state.spawns.length ? state : { spawns };
};
