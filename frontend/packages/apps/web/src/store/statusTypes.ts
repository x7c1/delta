import type { RateLimitWindow } from '@delta/wire-gen';

/**
 * The most recent account-wide rate-limit snapshot, taken from the latest
 * `status_updated` event of any session. Rate limits are account-wide (the same
 * across every session), so a single global value is kept and replaced on each
 * event rather than stored per session. Either window can be absent (a
 * non-Pro/Max account, or before the first API response of the day), in which
 * case that window is `null` and its footer row is hidden.
 *
 * This shape lives here rather than in the store so the persistence layer can
 * reference it without importing the store (which itself imports the
 * persistence layer — a cycle the linter rejects).
 */
export interface RateLimits {
  /** The rolling 5-hour window, or `null` when the account reports none. */
  fiveHour: RateLimitWindow | null;
  /** The rolling 7-day window, or `null` when the account reports none. */
  sevenDay: RateLimitWindow | null;
}
