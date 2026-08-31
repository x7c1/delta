import type { StateCreator } from 'zustand';
import type { SessionId } from '@delta/model';
import type { StatusSnapshot } from '@delta/wire-gen';
import type {
  RateLimitsByProvider,
  RateLimitsObservedAt,
  StatusObservedAt,
} from '../statusTypes';
import {
  loadPersistedStatus,
  savePersistedStatus,
  staleRateLimitsObservedAt,
} from '../statusPersistence';
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
  /**
   * When each entry above was observed (epoch ms). Bookkeeping for the
   * persistence layer, which expires every datum against its own observation
   * rather than against one snapshot-wide stamp — see `statusPersistence`. No
   * component reads this; it exists so a save triggered by one key carries
   * every other key's time forward untouched.
   */
  statusObservedAt: StatusObservedAt;
  /**
   * The providers whose rate-limit windows came from `localStorage` on load
   * carrying an observation older than `RATE_LIMIT_FRESHNESS_MS`, and that no
   * live snapshot has restated since, mapped to when they were last observed.
   * The navigator footer renders these rows de-emphasized and dates them,
   * rather than hiding them: an unreset window's percentage is still a
   * meaningful lower bound (other devices can only have added to it).
   *
   * Only the aged-out providers are marked, not every restored one — see
   * `staleRateLimitsObservedAt`, which owns that decision. A reload moments
   * after the last turn restores a reading that is still exactly right, and
   * nothing would undim it before the user's next agent turn anyway.
   *
   * Rate limits only. A context-usage percentage belongs to a session, and a
   * session emitting no live snapshot has no agent running to change it — the
   * restored value is exact, and a closed session would never send the event
   * that clears such a mark, so it would sit greyed out indefinitely.
   */
  restoredRateLimitsObservedAt: RateLimitsObservedAt;
}

// Seed the status slices from the last persisted snapshot (each datum
// expiry-checked on its own terms), so a reload restores the context bar /
// rate-limit footer instead of going blank until the next usage event. The
// load instant is kept so the staleness decision below is taken against the
// same clock reading the expiry checks used.
const restoredAt = Date.now();
const restoredStatus = loadPersistedStatus(restoredAt);

export const createStatusSlice: StateCreator<
  StatusSlice,
  [],
  [],
  StatusSlice
> = () => ({
  contextUsage: restoredStatus.contextUsage,
  rateLimits: restoredStatus.rateLimits,
  statusObservedAt: restoredStatus.observedAt,
  // The helper returns its own object rather than aliasing
  // `statusObservedAt.rateLimits`: the two maps diverge as live snapshots
  // arrive (one is refreshed, the other is emptied), so they must not begin
  // life sharing one.
  restoredRateLimitsObservedAt: staleRateLimitsObservedAt(
    restoredStatus.observedAt.rateLimits,
    restoredAt,
  ),
});

// A usage snapshot arrived. These fire frequently, so both pieces are stored
// replace-latest (never appended): the session's context-usage percentage
// replaces that session's previous value, and the account's rate limits replace
// that PROVIDER's previous windows.
//
// A snapshot need not speak to both: a provider that reports token usage and
// account limits on separate frames (Codex) sends one snapshot per frame, so
// each field is applied only when the snapshot actually stated it. The same
// "apply only what was stated" rule governs the observation bookkeeping — a
// usage-only frame says nothing about the account's limits, so it neither
// re-dates them nor clears their restored mark.
export const reduceStatusUpdated: EventReducer<StatusSlice, 'status_updated'> = (
  state,
  event,
) => {
  const snapshot: StatusSnapshot = event.snapshot;
  const now = Date.now();
  const next: Partial<StatusSlice> = {};

  let contextUsage = state.contextUsage;
  let contextObservedAt = state.statusObservedAt.contextUsage;

  // Context usage is per session. A `null` percentage (e.g. right after
  // `/compact`, before the next API response, or from a provider that reports
  // counts without a window size) drops the session's entry so the bar is
  // hidden rather than pinned at the old value or 0%.
  const pct = snapshot.context_used_percentage;
  if (pct === null) {
    if (contextUsage[event.session_id] !== undefined) {
      contextUsage = { ...contextUsage };
      delete contextUsage[event.session_id];
    }
    if (contextObservedAt[event.session_id] !== undefined) {
      contextObservedAt = { ...contextObservedAt };
      delete contextObservedAt[event.session_id];
    }
  } else {
    if (contextUsage[event.session_id] !== pct) {
      contextUsage = { ...contextUsage, [event.session_id]: pct };
    }
    // Re-dated even when the percentage is unchanged: the freshness of a
    // reading is about when it was last STATED, not when it last moved.
    contextObservedAt = { ...contextObservedAt, [event.session_id]: now };
  }
  if (contextUsage !== state.contextUsage) {
    next.contextUsage = contextUsage;
  }

  let rateLimits = state.rateLimits;
  let rateLimitsObservedAt = state.statusObservedAt.rateLimits;

  // Rate limits are account-wide, so they are keyed by the snapshot's provider
  // and replace only that provider's windows. `null` means the snapshot made no
  // statement about them; `[]` means the account has none (and the rows clear).
  if (snapshot.rate_limits !== null) {
    rateLimits = { ...rateLimits, [snapshot.provider]: snapshot.rate_limits };
    rateLimitsObservedAt = { ...rateLimitsObservedAt, [snapshot.provider]: now };
    next.rateLimits = rateLimits;
    // The server has now spoken for this account, so its rows stop being a
    // restored guess — and only this provider's do.
    if (state.restoredRateLimitsObservedAt[snapshot.provider] !== undefined) {
      const restored = { ...state.restoredRateLimitsObservedAt };
      delete restored[snapshot.provider];
      next.restoredRateLimitsObservedAt = restored;
    }
  }

  const statusObservedAt: StatusObservedAt = {
    contextUsage: contextObservedAt,
    rateLimits: rateLimitsObservedAt,
  };
  if (
    contextObservedAt !== state.statusObservedAt.contextUsage ||
    rateLimitsObservedAt !== state.statusObservedAt.rateLimits
  ) {
    next.statusObservedAt = statusObservedAt;
  }

  // Persist the latest snapshot so a reload can restore it (each datum
  // expiry-checked on its own terms in statusPersistence) instead of going
  // blank.
  savePersistedStatus({ contextUsage, rateLimits, observedAt: statusObservedAt });

  return next;
};
