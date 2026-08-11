import { beforeEach, describe, expect, it } from 'vitest';
import type { RateLimitWindow } from '@delta/wire-gen';
import type { RateLimitsByProvider } from './statusTypes';
import {
  loadPersistedStatus,
  savePersistedStatus,
} from './statusPersistence';

const STORAGE_KEY = 'delta:status-snapshot';
const TTL_MS = 60 * 60 * 1000; // 1 hour — mirrors statusPersistence.

// A fixed "now" (epoch ms) so every case drives the clock explicitly rather
// than relying on the real one. Its seconds value is what `resets_at` (epoch
// seconds) is compared against.
const NOW_MS = 1_700_000_000_000;
const NOW_SECONDS = NOW_MS / 1000;

const FIVE_HOURS = 5 * 60 * 60;
const SEVEN_DAYS = 7 * 24 * 60 * 60;

/** A window of `durationSeconds`, as the wire delivers it. */
function window(
  durationSeconds: number | null,
  usedPercentage: number | null,
  resetsAt: number | null,
): RateLimitWindow {
  return {
    duration_seconds: durationSeconds,
    used_percentage: usedPercentage,
    resets_at: resetsAt,
  };
}

/** A Claude account's two live windows — the common persisted shape. */
function rateLimits(
  overrides: RateLimitsByProvider = {},
): RateLimitsByProvider {
  return {
    claude: [
      window(FIVE_HOURS, 37, NOW_SECONDS + 3600),
      window(SEVEN_DAYS, 8, NOW_SECONDS + 5 * 86400),
    ],
    ...overrides,
  };
}

describe('statusPersistence', () => {
  beforeEach(() => {
    // jsdom provides localStorage; clear it so cases never leak into each other.
    localStorage.clear();
  });

  it('round-trips a saved snapshot back through load', () => {
    const contextUsage = { 'sess-1': 62, 'sess-2': 9 };
    const limits = rateLimits();

    savePersistedStatus(contextUsage, limits, NOW_MS);

    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage,
      rateLimits: limits,
    });
  });

  it('round-trips each provider\'s windows separately', () => {
    // Two accounts persisted side by side must come back side by side: a
    // restore that merged them would show one provider's limits under the
    // other's session, which is the whole thing the provider key prevents.
    const limits = rateLimits({
      codex: [window(SEVEN_DAYS, 44, NOW_SECONDS + 86400)],
    });

    savePersistedStatus({}, limits, NOW_MS);

    expect(loadPersistedStatus(NOW_MS).rateLimits).toEqual(limits);
  });

  it('discards a snapshot older than the TTL', () => {
    savePersistedStatus({ 'sess-1': 62 }, rateLimits(), NOW_MS);

    // Loaded one millisecond past the TTL: the whole snapshot is dropped and
    // the empty result is returned (the bar/footer start blank rather than
    // showing stale "mystery" values after a long-idle restart).
    const result = loadPersistedStatus(NOW_MS + TTL_MS + 1);
    expect(result).toEqual({ contextUsage: {}, rateLimits: {} });
  });

  it('keeps a snapshot that is exactly at the TTL boundary', () => {
    const contextUsage = { 'sess-1': 62 };
    savePersistedStatus(contextUsage, rateLimits(), NOW_MS);

    // Exactly TTL old is still fresh (the guard drops only strictly older
    // snapshots). Read at the same instant the windows reset against, so both
    // are still in the future and survive.
    const result = loadPersistedStatus(NOW_MS + TTL_MS);
    expect(result.contextUsage).toEqual(contextUsage);
    expect(result.rateLimits.claude).toHaveLength(2);
  });

  it('prunes rate-limit windows whose reset time has passed, keeping valid ones', () => {
    const stale = window(FIVE_HOURS, 90, NOW_SECONDS - 60);
    const live = window(SEVEN_DAYS, 8, NOW_SECONDS + 86400);
    // The 5h window already rolled over (its reset is in the past), so its
    // percentage is meaningless and it must be dropped; the 7d one survives.
    savePersistedStatus({}, { claude: [stale, live] }, NOW_MS);

    const result = loadPersistedStatus(NOW_MS);
    expect(result.rateLimits).toEqual({ claude: [live] });
  });

  it('leaves a provider with no live windows rather than dropping it', () => {
    savePersistedStatus(
      { 'sess-1': 62 },
      {
        claude: [
          window(FIVE_HOURS, 90, NOW_SECONDS - 60),
          window(SEVEN_DAYS, 8, NOW_SECONDS - 120),
        ],
      },
      NOW_MS,
    );

    const result = loadPersistedStatus(NOW_MS);
    // Context usage still restores; only the stale windows are gone. An empty
    // list and an absent provider render identically (no rows), so there is
    // nothing to gain from collapsing one into the other.
    expect(result.contextUsage).toEqual({ 'sess-1': 62 });
    expect(result.rateLimits).toEqual({ claude: [] });
  });

  it('keeps a window with a null reset time (no timestamp to expire against)', () => {
    const undated = window(FIVE_HOURS, 50, null);
    savePersistedStatus({}, { claude: [undated] }, NOW_MS);

    expect(loadPersistedStatus(NOW_MS).rateLimits).toEqual({
      claude: [undated],
    });
  });

  it('returns the empty result when nothing is persisted', () => {
    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage: {},
      rateLimits: {},
    });
  });

  it('returns the empty result (without throwing) for garbage in localStorage', () => {
    localStorage.setItem(STORAGE_KEY, 'not json at all {');

    expect(() => loadPersistedStatus(NOW_MS)).not.toThrow();
    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage: {},
      rateLimits: {},
    });
  });

  it('returns the empty result for the previous release\'s persisted shape', () => {
    // Every user upgrading into this change has yesterday's payload sitting in
    // localStorage, where `rateLimits` was a single `{ fiveHour, sevenDay }`
    // object rather than per-provider lists. Loading it must degrade to the
    // empty result — this module is read at store-module load, so a throw that
    // escaped here would be a blank app on first load after the upgrade, not
    // just a blank footer.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        savedAt: NOW_MS,
        contextUsage: { 'sess-1': 62 },
        rateLimits: {
          fiveHour: { used_percentage: 37, resets_at: NOW_SECONDS + 3600 },
          sevenDay: { used_percentage: 8, resets_at: NOW_SECONDS + 86400 },
        },
      }),
    );

    expect(() => loadPersistedStatus(NOW_MS)).not.toThrow();
    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage: {},
      rateLimits: {},
    });
  });

  it('returns the empty result for a snapshot missing its savedAt stamp', () => {
    // A payload without a numeric `savedAt` cannot be freshness-checked, so it
    // is treated as stale rather than trusted.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ contextUsage: { 'sess-1': 5 }, rateLimits: rateLimits() }),
    );

    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage: {},
      rateLimits: {},
    });
  });
});
