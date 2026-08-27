import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { UnsentSend } from '@delta/wire-gen';
import { NEW_SESSION_DRAFT_KEY, useComposerStore } from '../composerStore';
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
  /**
   * The `send` row id the POST returned for {@link SpawnItem.text}. It is what
   * splits a failed launch's `spawn_failed.unsent` list — see
   * {@link restoreUnsentIntoDraft}.
   */
  firstSendId: number;
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
  /**
   * How many of the launch's undelivered messages went back into the
   * new-session composer draft (see {@link restoreUnsentIntoDraft}) — the ones
   * queued *behind* this spawn's own first prompt, which stays on the chip for
   * Retry to re-send. `undefined` while spawning, `0` for a failure that had
   * nothing behind the first prompt. The failed chip reads it to say where
   * those messages went, which nothing else on screen can: they are in a
   * composer the user may not be looking at, and Retry does not re-send them.
   */
  restoredCount?: number;
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

/**
 * Put back the messages a failed launch never delivered, minus the one the
 * failed chip already holds. Returns how many were restored.
 *
 * The server deletes a failed spawn's `send` rows, so `spawn_failed.unsent` is
 * the only remaining copy of what the user wrote. The spawn's own first prompt
 * is excluded by id: it lives on the {@link SpawnItem}, and the Retry chip
 * re-sends exactly that — restoring it here too would duplicate it. Everything
 * typed after it has no other home, so it goes back to the new-session
 * composer. `firstSendId` is `null` when there is no chip to hold anything back
 * (see {@link reduceSpawnFailed}'s untracked branch) and then the whole list is
 * restored, first prompt included.
 *
 * **Appended, never assigned.** The draft may already hold something the user
 * typed while the launch was failing, and clobbering that would destroy the
 * newer text to restore the older. The restored messages join it below,
 * separated by a blank line, oldest first — the order they were composed in.
 *
 * Nothing is re-sent: the text simply waits in the composer for the user to
 * send it deliberately, the same decision the Retry chip embodies for the first
 * prompt.
 */
function restoreUnsentIntoDraft(
  unsent: UnsentSend[],
  firstSendId: number | null,
): number {
  const texts = unsent
    .filter((send) => send.send_id !== firstSendId)
    .map((send) => send.text);
  if (texts.length === 0) {
    return 0;
  }
  const composer = useComposerStore.getState();
  const existing = composer.drafts[NEW_SESSION_DRAFT_KEY] ?? '';
  composer.setDraft(
    NEW_SESSION_DRAFT_KEY,
    [...(existing.length > 0 ? [existing] : []), ...texts].join('\n\n'),
  );
  return texts.length;
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
 * Tell the user, through the app-wide snackbar, that a launch they had no chip
 * for has failed — and what became of the text they had typed into it.
 *
 * The per-session {@link SessionNotice} kinds cannot carry this: every one of
 * them renders inside its session's transcript pane, and this session's row is
 * being deleted as the event arrives (the user is handed to the new-session
 * surface). The snackbar is the only surface that outlives the session, which
 * is why it — not a notice — is what this path raises.
 */
function reportUntrackedSpawnFailure(
  reason: string | undefined,
  restored: number,
): void {
  const parts: string[] = [];
  if (reason !== undefined) {
    parts.push(reason);
  }
  if (restored > 0) {
    parts.push(
      restored === 1
        ? 'The unsent message was returned to the composer.'
        : `The ${restored} unsent messages were returned to the composer.`,
    );
  }
  useNotificationStore
    .getState()
    .showError(
      'The session failed to start',
      parts.length > 0 ? parts.join(' — ') : undefined,
    );
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
    // The failure outran the POST response. Only now is the spawn's own first
    // send id known, so this is the first moment the buffered `unsent` list can
    // be split — hence the restore here rather than when the event landed. A
    // buffer already flagged `restored` was put back whole by the event itself
    // and must not go in twice.
    const restoredCount = buffered.restored
      ? 0
      : restoreUnsentIntoDraft(buffered.unsent, spawn.firstSendId);
    // Register the spawn already failed (the Retry/Dismiss chip surfaces right
    // away, with the reason the buffered failure carried), consume the buffered
    // failure, and drop the just-recorded local send for it — its turn will
    // never end.
    set((state) => ({
      spawns: [
        ...state.spawns,
        { ...spawn, status: 'failed', reason: buffered.reason, restoredCount },
      ],
      ...removeNotices(
        state.notices,
        spawn.sessionId,
        (notice) => notice.kind === 'spawn_failure_buffered',
      ),
      ...dropLocalSendsForSession(state, spawn.sessionId),
    }));
  },

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
// tracked spawn at all splits two ways, told apart by whether a
// new-session POST is still travelling (see `newSessionPostInFlight`):
// during that window the failure merely outran this client's own POST
// response, so it is buffered for the `trackSpawn` that consumes it
// moments later; outside it, no registration is ever coming — the
// registry is in-memory, so a browser reload leaves a launch it started
// entirely unknown to it — and the text must be rescued HERE or nowhere,
// because the `send` rows are already deleted and `unsent` is their last
// copy. A spawn this client never started at all (a second tab's, another
// browser's) has no registration coming either, so it lands on that same
// branch: its text goes into THIS window's composer and its failure into
// THIS window's snackbar. Deliberate — the event carries nothing that
// tells the two apart, and text surfaced in the wrong window beats text
// dropped in silence.
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
    // with below, changes nothing — and must not restore the same text twice.
    if (
      state.spawns.some((spawn) => spawn.sessionId === event.session_id) ||
      noticeOf(state.notices, event.session_id, 'spawn_failure_buffered')
    ) {
      return state;
    }
    if (newSessionPostInFlight(state)) {
      // Hold everything for `trackSpawn`: which entry is the spawn's own first
      // prompt is unknowable until the POST response names its send id, and
      // splitting the list wrongly would either duplicate that prompt (chip and
      // composer both) or drop a message the user has no other copy of.
      return {
        notices: withNotice(state.notices, event.session_id, {
          kind: 'spawn_failure_buffered',
          reason: event.reason,
          unsent: event.unsent,
          restored: false,
        }),
      };
    }
    const restored = restoreUnsentIntoDraft(event.unsent, null);
    reportUntrackedSpawnFailure(event.reason, restored);
    // The buffered entry stays behind purely as the "already handled" marker
    // the guard above reads.
    return {
      notices: withNotice(state.notices, event.session_id, {
        kind: 'spawn_failure_buffered',
        reason: event.reason,
        unsent: event.unsent,
        restored: true,
      }),
    };
  }
  const spawns = state.spawns.slice();
  // A deliberate write into the composer store from a reducer: this is the one
  // place that knows both the event's list and the tracked spawn's first send
  // id.
  const restoredCount = restoreUnsentIntoDraft(
    event.unsent,
    spawns[idx].firstSendId,
  );
  spawns[idx] = {
    ...spawns[idx],
    status: 'failed',
    reason: event.reason,
    restoredCount,
  };
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
