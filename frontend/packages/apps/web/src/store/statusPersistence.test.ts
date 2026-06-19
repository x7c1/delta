import { beforeEach, describe, expect, it } from 'vitest';
import type { RateLimits } from './statusTypes';
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

function rateLimits(overrides: Partial<RateLimits> = {}): RateLimits {
  return {
    fiveHour: { used_percentage: 37, resets_at: NOW_SECONDS + 3600 },
    sevenDay: { used_percentage: 8, resets_at: NOW_SECONDS + 5 * 86400 },
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

  it('discards a snapshot older than the TTL', () => {
    savePersistedStatus({ 'sess-1': 62 }, rateLimits(), NOW_MS);

    // Loaded one millisecond past the TTL: the whole snapshot is dropped and
    // the empty result is returned (the bar/footer start blank rather than
    // showing stale "mystery" values after a long-idle restart).
    const result = loadPersistedStatus(NOW_MS + TTL_MS + 1);
    expect(result).toEqual({ contextUsage: {}, rateLimits: null });
  });

  it('keeps a snapshot that is exactly at the TTL boundary', () => {
    const contextUsage = { 'sess-1': 62 };
    savePersistedStatus(contextUsage, rateLimits(), NOW_MS);

    // Exactly TTL old is still fresh (the guard drops only strictly older
    // snapshots). Read at the same instant the windows reset against, so both
    // are still in the future and survive.
    const result = loadPersistedStatus(NOW_MS + TTL_MS);
    expect(result.contextUsage).toEqual(contextUsage);
    expect(result.rateLimits).not.toBeNull();
  });

  it('prunes rate-limit windows whose reset time has passed, keeping valid ones', () => {
    const limits = rateLimits({
      // The 5h window already rolled over (its reset is in the past), so its
      // percentage is meaningless and it must be dropped.
      fiveHour: { used_percentage: 90, resets_at: NOW_SECONDS - 60 },
      // The 7d window is still valid and must survive.
      sevenDay: { used_percentage: 8, resets_at: NOW_SECONDS + 86400 },
    });
    savePersistedStatus({}, limits, NOW_MS);

    const result = loadPersistedStatus(NOW_MS);
    expect(result.rateLimits).toEqual({
      fiveHour: null,
      sevenDay: limits.sevenDay,
    });
  });

  it('collapses rateLimits to null when every window has expired', () => {
    const limits = rateLimits({
      fiveHour: { used_percentage: 90, resets_at: NOW_SECONDS - 60 },
      sevenDay: { used_percentage: 8, resets_at: NOW_SECONDS - 120 },
    });
    savePersistedStatus({ 'sess-1': 62 }, limits, NOW_MS);

    const result = loadPersistedStatus(NOW_MS);
    // Context usage still restores; only the stale windows are gone.
    expect(result.contextUsage).toEqual({ 'sess-1': 62 });
    expect(result.rateLimits).toBeNull();
  });

  it('keeps a window with a null reset time (no timestamp to expire against)', () => {
    const limits = rateLimits({
      fiveHour: { used_percentage: 50, resets_at: null },
      sevenDay: null,
    });
    savePersistedStatus({}, limits, NOW_MS);

    const result = loadPersistedStatus(NOW_MS);
    expect(result.rateLimits).toEqual({
      fiveHour: { used_percentage: 50, resets_at: null },
      sevenDay: null,
    });
  });

  it('returns the empty result when nothing is persisted', () => {
    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage: {},
      rateLimits: null,
    });
  });

  it('returns the empty result (without throwing) for garbage in localStorage', () => {
    localStorage.setItem(STORAGE_KEY, 'not json at all {');

    expect(() => loadPersistedStatus(NOW_MS)).not.toThrow();
    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage: {},
      rateLimits: null,
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
      rateLimits: null,
    });
  });
});
