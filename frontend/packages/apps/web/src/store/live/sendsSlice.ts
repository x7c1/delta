import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';

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
   * {@link endTurnForSession} detects. The POST's caller (the submit
   * hook) reads this flag in `onSuccess`: if set, the send is dropped instead
   * of staged into {@link SendsSlice.localSends}, so a chip with no remaining
   * drain trigger never lands. A normal POST without a racing turn-end never
   * carries the flag, so the standard `recordLocalSend` path runs.
   *
   * Storing the race signal directly on the in-flight submit keeps the race
   * detection scoped to the same record that already represents "this POST is
   * mid-flight" — no separate per-session counter that has to be kept in step
   * with the {@link SendsSlice.sending} array.
   */
  dropOnResolve?: true;
  /**
   * What the server said was wrong, when the rejection named something the
   * generic failure copy cannot say — today a launch option the provider's
   * adapter refuses (`launch_option_rejected`), whose message names the
   * offending field or config key path. Shown verbatim under the failed chip's
   * headline, the way a `spawn_failed` reason is; absent for every failure
   * whose message says no more than "it failed".
   */
  reason?: string;
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

export interface SendsSlice {
  /** Submits not yet accepted (or rejected) by the server, oldest first. */
  sending: SendingItem[];
  /**
   * Accepted sends whose turn has not ended, keyed by send id. Rendered as an
   * in-progress chip only while the send is absent from the server's open
   * list (i.e. it already matched); drained by the turn-end events.
   */
  localSends: Record<number, LocalSend>;

  /** Record a submit whose `POST /api/sends` is about to fly. */
  beginSending: (item: SendingItem) => void;
  /**
   * Mark an in-flight submit rejected, surfacing the recoverable chip.
   * `reason` is the server's own message when it named something specific (see
   * {@link SendingItem.reason}).
   */
  failSending: (id: string, reason?: string) => void;
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
}

/**
 * Drop the tracked local sends of one session, returning the changed slice
 * (empty object when nothing matched). Used when the session's spawn failed —
 * its turn will never end, so nothing else drains those sends.
 */
export function dropLocalSendsForSession(
  state: Pick<SendsSlice, 'localSends'>,
  sessionId: SessionId,
): Partial<Pick<SendsSlice, 'localSends'>> {
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
export function flagRacedSendingForSession(
  state: Pick<SendsSlice, 'sending'>,
  sessionId: SessionId,
): Partial<Pick<SendsSlice, 'sending'>> {
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

export const createSendsSlice: StateCreator<SendsSlice, [], [], SendsSlice> = (
  set,
) => ({
  sending: [],
  localSends: {},

  beginSending: (item) =>
    set((state) => ({ sending: [...state.sending, item] })),

  failSending: (id, reason) =>
    set((state) => ({
      sending: state.sending.map((item) =>
        item.id === id ? { ...item, status: 'failed', reason } : item,
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
});
