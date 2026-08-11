import type { StateCreator } from 'zustand';
import type { SessionId } from '@delta/model';
import type { StatusSnapshot } from '@delta/wire-gen';
import type { RateLimitsByProvider } from '../statusTypes';
import { loadPersistedStatus, savePersistedStatus } from '../statusPersistence';
import type { EventReducer } from './eventReducer';

export interface StatusSlice {
  /**
   * The latest context-usage percentage per session, keyed by session id.
   * Replace-latest, not append: usage snapshots fire frequently, so only the
   * most recent one of each session is kept. Drives the composer's top-edge
   * context bar for the focused session. A session with no percentage yet has
   * no entry, so the bar is hidden rather than shown at 0%: either no snapshot
   * has arrived, or the percentage was `null` — right after `/compact`, or from
   * a provider that reports no context-window size and so cannot compute one.
   */
  contextUsage: Record<SessionId, number>;
  /**
   * The account-wide rate-limit windows, keyed by provider. See
   * {@link RateLimitsByProvider} for why the key is load-bearing: the navigator
   * footer renders the windows of the FOCUSED session's provider, so one
   * provider's account limits can never be presented as another's.
   */
  rateLimits: RateLimitsByProvider;
}

// Seed the status slices from the last persisted snapshot (freshness-guarded),
// so a reload restores the context bar / rate-limit footer instead of going
// blank until the next usage event.
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

// A usage snapshot arrived. These fire frequently, so both pieces are stored
// replace-latest (never appended): the session's context-usage percentage
// replaces that session's previous value, and the account's rate limits replace
// that PROVIDER's previous windows.
//
// A snapshot need not speak to both: a provider that reports token usage and
// account limits on separate frames (Codex) sends one snapshot per frame, so
// each field is applied only when the snapshot actually stated it.
export const reduceStatusUpdated: EventReducer<StatusSlice, 'status_updated'> = (
  state,
  event,
) => {
  const snapshot: StatusSnapshot = event.snapshot;
  const next: Partial<StatusSlice> = {};

  // Context usage is per session. A `null` percentage (e.g. right after
  // `/compact`, before the next API response, or from a provider that reports
  // counts without a window size) drops the session's entry so the bar is
  // hidden rather than pinned at the old value or 0%.
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

  // Rate limits are account-wide, so they are keyed by the snapshot's provider
  // and replace only that provider's windows. `null` means the snapshot made no
  // statement about them; `[]` means the account has none (and the rows clear).
  if (snapshot.rate_limits !== null) {
    next.rateLimits = {
      ...state.rateLimits,
      [snapshot.provider]: snapshot.rate_limits,
    };
  }

  // Persist the latest snapshot so a reload can restore it (freshness-
  // guarded in statusPersistence) instead of going blank.
  savePersistedStatus(
    next.contextUsage ?? state.contextUsage,
    next.rateLimits ?? state.rateLimits,
    Date.now(),
  );

  return next;
};
