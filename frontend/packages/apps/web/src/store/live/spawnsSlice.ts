import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import { useNotificationStore } from '../notificationStore';
import type { EventReducer } from './eventReducer';
import type { NoticesSlice } from './noticesSlice';
import { noticeOf, removeNotices, withNotice } from './noticesSlice';
import type { NewSessionLaunch, SendsSlice } from './sendsSlice';
import { dropLocalSendsForSession } from './sendsSlice';

/**
 * The state this module's action and reducer read: its own spawn registry,
 * plus the notices map (the buffered early failure lives there), the tracked
 * local sends (a failed spawn's send is dropped — its turn never ends), and the
 * in-flight submits (an unrecognized failure is told apart from a racing POST
 * by them — see {@link newSessionPostInFlight}).
 */
type SpawnsState = Pick<SpawnsSlice, 'spawns'> &
  Pick<NoticesSlice, 'notices'> &
  Pick<SendsSlice, 'localSends' | 'sending'>;

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
  /** The `send` row id the POST returned for {@link SpawnItem.text}. */
  firstSendId: number;
  /**
   * spawning: launch in flight; failed: the launch ended without ever binding
   * (`spawn_failed` arrived) — it broke, the watchdog reaped it, or the user
   * cancelled it.
   *
   * A `failed` entry renders nothing of its own. The server keeps the failed
   * session's row, so the failure has its own screen, fed by the session list
   * and the session's open sends — the reason included, which is why no copy
   * of it is kept here. The entry survives for the two things the row does not
   * carry: the **launch configuration** (the launch-option ids and the worktree
   * request are persisted nowhere) that Retry re-attempts the identical launch
   * from, and {@link SpawnItem.cancelled}. It is dropped once the user retries
   * or removes the session.
   */
  status: 'spawning' | 'failed';
  /**
   * True when the user asked for the launch to stop: they closed a session
   * that was still starting. A cancel is not a breakage, so anything that
   * words the ending says so. `undefined` while spawning.
   */
  cancelled?: boolean;
  /**
   * True once this spawn's focus hand-over is over — used or spent. The
   * hand-over is a one-shot and this is its record, set either by the
   * workspace as it moves focus onto the spawn, or already at
   * {@link SpawnsSlice.trackSpawn} time by `useSubmitSend`, which spends the
   * hand-over of a POST that landed with the user elsewhere and holds the
   * reasoning for deciding it there. `WorkspaceScreen` holds the effect and
   * the reasoning for why the focused screen alone cannot stand in for that
   * record.
   *
   * Never read on a `failed` spawn, however this flag stands there: the
   * hand-over only ever considers `spawning` ones — so a spawn registered
   * already `failed` (a buffered failure) simply carries whatever the caller
   * decided a moment before the failure was known.
   *
   * `undefined` only in state seeded directly, as tests do, which reads as not
   * handed over; every `trackSpawn` decides it.
   */
  focusHandedOver?: boolean;
}

export interface SpawnsSlice {
  /** Tracked new-session spawns, oldest first, keyed by real session id. */
  spawns: SpawnItem[];

  /**
   * Track a new-session spawn (real ids from the POST response). If the
   * spawn's failure already arrived (see {@link SpawnFailureBufferedNotice}),
   * the spawn is registered as `failed` immediately.
   *
   * The caller supplies {@link SpawnItem.focusHandedOver}: it is the one place
   * that knows where the user was when the server answered.
   */
  trackSpawn: (
    spawn: Omit<SpawnItem, 'status' | 'focusHandedOver'> & {
      focusHandedOver: boolean;
    },
  ) => void;
  /**
   * Record that this spawn's focus hand-over is over, so it is never handed
   * over again (see {@link SpawnItem.focusHandedOver}). Called by the workspace
   * for the spawn it moves focus onto, and for any sibling that was waiting
   * alongside it; a no-op for an id that is not tracked, or whose hand-over is
   * already recorded.
   */
  markSpawnFocusHandedOver: (sessionId: SessionId) => void;
  /**
   * Drop a tracked spawn. A spawn that comes up is released by its
   * `session_registered` event (see {@link reduceSessionRegistered}), so this
   * is the manual path: a failed spawn retried, or its session removed.
   */
  clearSpawn: (sessionId: SessionId) => void;
}

/**
 * Whether a new-session `POST /api/sends` is still travelling, i.e. a
 * {@link SpawnsSlice.trackSpawn} for an id this client cannot know yet is
 * moments away. A new-session submit records its `sending` chip *before* the
 * POST leaves and drops it in the response handler, so this is exactly the
 * window in which a `spawn_failed` naming an untracked session can still be
 * this client's own spawn — the same reasoning `flagRacedSendingForSession`
 * uses for a racing turn-end. Outside that window an untracked id will never
 * become tracked, however long anything waits for it.
 */
function newSessionPostInFlight(state: Pick<SendsSlice, 'sending'>): boolean {
  return state.sending.some(
    (item) => item.status === 'sending' && item.target.kind === 'new-session',
  );
}

/**
 * Tell the user, through the app-wide snackbar, how a launch this client never
 * tracked ended — it broke, or somebody cancelled it.
 *
 * A tracked launch needs no snackbar: its session is listed, marked failed, and
 * its own screen says what happened. An untracked one has no such connection to
 * this window — it was started by another tab, or before a reload — so the
 * snackbar is what says a launch somewhere ended, and the failed row in the
 * navigator is what the user opens to read the rest.
 *
 * One producer of this event leaves no such row: a *resume* that never became
 * ready fails a session that was already bound once, so the server closes it
 * rather than marking it `failed`. Such a failure is always untracked (only a
 * new-session POST ever registers a spawn), and the snackbar is the whole of
 * what the user gets.
 */
function reportUntrackedSpawnFailure(
  reason: string | undefined,
  cancelled: boolean,
): void {
  const notifications = useNotificationStore.getState();
  // The headline alone. Since the watchdog began quoting the pane, a reason can
  // be a line of explanation, a blank line, and then a dozen lines of a TUI —
  // and the snackbar is a fixed-width box that dismisses itself after a few
  // seconds, so a captured screen pasted into it is a wall of text nobody can
  // read in time. It is also no longer where that content belongs: the failed
  // session's own screen shows the reason in full, and nothing else here is
  // narrowed. A single-line reason is its own first line, and an absent one
  // stays absent.
  const headline = reason?.split('\n', 1)[0];
  // A cancel is something the user asked for, so it states what happened
  // instead of alarming: only a launch that broke on its own is an error.
  if (cancelled) {
    notifications.showInfo('Launch cancelled', headline);
    return;
  }
  notifications.showError('The session failed to start', headline);
}

export const createSpawnsSlice: StateCreator<
  SpawnsState & SpawnsSlice,
  [],
  [],
  SpawnsSlice
> = (set, get) => ({
  spawns: [],

  trackSpawn: (spawn) => {
    const buffered = noticeOf(
      get().notices,
      spawn.sessionId,
      'spawn_failure_buffered',
    );
    if (!buffered) {
      set((state) => ({
        spawns: [...state.spawns, { ...spawn, status: 'spawning' }],
      }));
      return;
    }
    // The failure outran the POST response. Register the spawn already
    // `failed` — so the workspace never hands focus to a launch that is
    // already over, and Retry on the failed session's screen still finds its
    // launch configuration — consume the buffered failure, and drop the
    // just-recorded local send for it, whose turn will never end.
    set((state) => ({
      spawns: [
        ...state.spawns,
        {
          ...spawn,
          status: 'failed',
          cancelled: buffered.cancelled,
        },
      ],
      ...removeNotices(
        state.notices,
        spawn.sessionId,
        (notice) => notice.kind === 'spawn_failure_buffered',
      ),
      ...dropLocalSendsForSession(state, spawn.sessionId),
    }));
  },

  markSpawnFocusHandedOver: (sessionId) =>
    set((state) => {
      const idx = state.spawns.findIndex(
        (spawn) => spawn.sessionId === sessionId && !spawn.focusHandedOver,
      );
      if (idx === -1) {
        // Already recorded, or nothing tracked under that id. Hand back the
        // identity-stable state so subscribers are not notified.
        return state;
      }
      const spawns = state.spawns.slice();
      spawns[idx] = { ...spawns[idx], focusHandedOver: true };
      return { spawns };
    }),

  clearSpawn: (sessionId) =>
    set((state) => ({
      spawns: state.spawns.filter((spawn) => spawn.sessionId !== sessionId),
    })),
});

// The launch ended without ever binding: it broke, the server reaped it, or
// the user closed the still-starting session and cancelled it (`event.cancelled`
// says which).
//
// The session row is KEPT, so nothing has to be rescued into this store: the
// tracked entry is flipped to `failed` and held for its launch configuration
// alone (see `SpawnItem.status`).
//
// Its tracked local send is dropped: that send's turn will never end, and the
// row behind it is now an ordinary open send of a failed session, shown by that
// session's own screen.
//
// The event carries the REAL session id the POST response returned, so the
// match is exact. An id with no tracked spawn at all splits two ways, told
// apart by whether a new-session POST is still travelling (see
// `newSessionPostInFlight`): during that window the failure merely outran this
// client's own POST response, so it is buffered for the `trackSpawn` that
// consumes it moments later; outside it, no registration is ever coming — the
// registry is in-memory, so a browser reload leaves a launch it started
// entirely unknown to it — and the snackbar is raised here instead. A spawn
// this client never started at all (a second tab's, another browser's) lands on
// that same branch; the failed row is in the navigator either way.
export const reduceSpawnFailed: EventReducer<SpawnsState, 'spawn_failed'> = (
  state,
  event,
) => {
  const idx = state.spawns.findIndex(
    (spawn) =>
      spawn.sessionId === event.session_id && spawn.status === 'spawning',
  );
  if (idx === -1) {
    // A repeat for a spawn already failed, or for an untracked id already dealt
    // with below, changes nothing — and must not raise a second snackbar.
    if (
      state.spawns.some((spawn) => spawn.sessionId === event.session_id) ||
      noticeOf(state.notices, event.session_id, 'spawn_failure_buffered')
    ) {
      return state;
    }
    if (newSessionPostInFlight(state)) {
      // Hold it for `trackSpawn`, which is moments away and is what turns the
      // POST response into a tracked entry: registering that entry as
      // `spawning` after its launch has already ended would hand focus to a
      // session that is never coming up.
      return {
        notices: withNotice(state.notices, event.session_id, {
          kind: 'spawn_failure_buffered',
          cancelled: event.cancelled,
        }),
      };
    }
    reportUntrackedSpawnFailure(event.reason, event.cancelled);
    // The buffered entry stays behind purely as the "already handled" marker
    // the guard above reads.
    return {
      notices: withNotice(state.notices, event.session_id, {
        kind: 'spawn_failure_buffered',
        cancelled: event.cancelled,
      }),
    };
  }
  const spawns = state.spawns.slice();
  spawns[idx] = {
    ...spawns[idx],
    status: 'failed',
    cancelled: event.cancelled,
  };
  return {
    spawns,
    ...dropLocalSendsForSession(state, event.session_id),
  };
};

/**
 * The spawn came up: its launch bound and the server activated the row. The
 * tracked entry has done its job — the workspace focused the session if the
 * user was still waiting on the new-session screen when the POST was accepted
 * — so drop it here. The entry feeds no pending row: the session's own strip
 * renders from its open sends and the local sends tracked alongside them.
 *
 * This is the release point precisely because it is the LAST thing the entry
 * is needed for: while a spawn is tracked the workspace refuses to reconcile
 * focus away from its id (the row may not be in the loaded page yet), which it
 * has to hold until the row is listed. Only a `spawning` entry is dropped: a
 * `failed` one still holds the launch configuration its session's Retry reads,
 * and a registration for a session this client never spawned matches nothing.
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

/**
 * The session was removed: whatever this client still tracked for it goes with
 * it.
 *
 * A failed spawn's entry outlives its launch on purpose (see
 * `SpawnItem.status`), so something has to end it when the session itself ends.
 * Removing it from the failed session's own screen clears the entry directly,
 * but the navigator's kebab (and another tab, and another browser) reach the
 * same `DELETE` without passing through that screen, and this event is what all
 * of them have in common.
 */
export const reduceSessionRemoved: EventReducer<
  SpawnsState,
  'session_removed'
> = (state, event) => {
  const spawns = state.spawns.filter(
    (spawn) => spawn.sessionId !== event.session_id,
  );
  // Nothing tracked under that id — the common case, since most removals are of
  // ordinary closed sessions. Hand back the identity-stable state so
  // subscribers are not notified.
  return spawns.length === state.spawns.length ? state : { spawns };
};
