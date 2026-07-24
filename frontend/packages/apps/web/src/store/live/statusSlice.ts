import type { StateCreator } from 'zustand';
import type { SessionId } from '@delta/model';
import type { StatusSnapshot } from '@delta/wire-gen';
import type { RateLimits } from '../statusTypes';
import { loadPersistedStatus, savePersistedStatus } from '../statusPersistence';
import type { EventReducer } from './eventReducer';

export interface StatusSlice {
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
}

// Seed the status slices from the last persisted snapshot (freshness-guarded),
// so a reload restores the context bar / rate-limit footer instead of going
// blank until the next statusLine event.
const restoredStatus = loadPersistedStatus(Date.now());

export const createStatusSlice: StateCreator<
  StatusSlice,
  [],
  [],
  StatusSlice
> = () => ({
  contextUsage: restoredStatus.contextUsage,
  rateLimits: restoredStatus.rateLimits,
});

// A Claude Code status-line snapshot arrived. These fire frequently,
// so both pieces are stored replace-latest (never appended): the
// session's context-usage percentage replaces that session's previous
// value, and the account-wide rate limits replace the single global
// snapshot (every session reports the same windows, so the most recent
// event wins regardless of which session it came from).
export const reduceStatusUpdated: EventReducer<StatusSlice, 'status_updated'> = (
  state,
  event,
) => {
  const snapshot: StatusSnapshot = event.snapshot;
  const next: Partial<StatusSlice> = {};

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
};
