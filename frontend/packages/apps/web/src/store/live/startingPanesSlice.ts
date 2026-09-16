import type { StateCreator } from 'zustand';
import type { SessionId } from '@delta/model';
import type { EventReducer } from './eventReducer';

/** The starting-pane set alone, the only field this module touches. */
type StartingPanesState = Pick<StartingPanesSlice, 'startingPanes'>;

export interface StartingPanesSlice {
  /**
   * The sessions whose tmux pane is up while their launch has not bound yet.
   *
   * The `spawn_pane_ready` event is the only thing that marks this window — the
   * session row reads `spawning` on both sides of it — and this set is where
   * the browser keeps it, read by the workspace to decide whether the embedded
   * terminal may attach to a session that is still starting. An entry is added
   * by that event and removed at either end of the window; see
   * {@link reduceStartingPaneEnded}.
   *
   * In-memory only, like the rest of this store, and nothing depends on it
   * being complete: a browser that missed the event (a reload mid-launch) just
   * does not offer the terminal until the session binds, which is what it did
   * before the event existed.
   */
  startingPanes: SessionId[];
}

export const createStartingPanesSlice: StateCreator<
  StartingPanesSlice,
  [],
  [],
  StartingPanesSlice
> = () => ({
  startingPanes: [],
});

/**
 * Whether `sessionId`'s pane is up while its launch has not bound yet — read
 * off {@link StartingPanesSlice.startingPanes}, taken as a plain value so a
 * component subscribes to that one field rather than to the store.
 */
export function paneIsStarting(
  startingPanes: SessionId[],
  sessionId: SessionId,
): boolean {
  return startingPanes.includes(sessionId);
}

/**
 * The launch's pane came up: it can be attached to from here, though nothing
 * has bound it. Idempotent — a repeated announcement (a reconnect replaying
 * nothing, a second tab's server) changes nothing.
 */
export const reduceSpawnPaneReady: EventReducer<
  StartingPanesState,
  'spawn_pane_ready'
> = (state, event) => {
  if (state.startingPanes.includes(event.session_id)) {
    return state;
  }
  return { startingPanes: [...state.startingPanes, event.session_id] };
};

/**
 * Either end of the starting window — the launch bound, or it is over — drops
 * the entry: from here the session's own row says whether it has a live pane.
 *
 * One reducer for both events because they are the same fact about this set (it
 * no longer applies), and it is keyed by session id, which both carry.
 */
export const reduceStartingPaneEnded: EventReducer<
  StartingPanesState,
  'session_registered' | 'spawn_failed'
> = (state, event) => {
  const startingPanes = state.startingPanes.filter(
    (sessionId) => sessionId !== event.session_id,
  );
  // Nothing matched — the common case, since most of these events are for
  // sessions that never had an entry. Hand back the identity-stable state so
  // subscribers are not notified.
  return startingPanes.length === state.startingPanes.length
    ? state
    : { startingPanes };
};
