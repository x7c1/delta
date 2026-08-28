import { beforeEach, describe, expect, it } from 'vitest';
import type { RateLimitWindow } from '@delta/wire-gen';
import type { RateLimitsByProvider, StatusObservedAt } from './statusTypes';
import {
  CONTEXT_USAGE_GC_TTL_MS,
  RATE_LIMIT_FRESHNESS_MS,
  loadPersistedStatus,
  savePersistedStatus,
  type CachedStatus,
} from './statusPersistence';

const STORAGE_KEY = 'delta:status-snapshot';

// A fixed "now" (epoch ms) so every case drives the clock explicitly rather
// than relying on the real one. Its seconds value is what `resets_at` (epoch
// seconds) is compared against.
const NOW_MS = 1_700_000_000_000;
const NOW_SECONDS = NOW_MS / 1000;

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

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

/**
 * A cache entry as the store would hand it to `savePersistedStatus`: values
 * plus, per key, when each was observed. `observedAt` defaults to stamping
 * every supplied key at `NOW_MS`, which is what a snapshot arriving now does.
 */
function cached(
  contextUsage: Record<string, number>,
  limits: RateLimitsByProvider,
  observedAt?: StatusObservedAt,
): CachedStatus {
  return {
    contextUsage,
    rateLimits: limits,
    observedAt: observedAt ?? {
      contextUsage: Object.fromEntries(
        Object.keys(contextUsage).map((key) => [key, NOW_MS]),
      ),
      rateLimits: Object.fromEntries(
        Object.keys(limits).map((key) => [key, NOW_MS]),
      ),
    },
  };
}

const EMPTY: CachedStatus = {
  contextUsage: {},
  rateLimits: {},
  observedAt: { contextUsage: {}, rateLimits: {} },
};

describe('statusPersistence', () => {
  beforeEach(() => {
    // jsdom provides localStorage; clear it so cases never leak into each other.
    localStorage.clear();
  });

  it('round-trips a saved snapshot back through load', () => {
    const contextUsage = { 'sess-1': 62, 'sess-2': 9 };
    const limits = rateLimits();

    savePersistedStatus(cached(contextUsage, limits));

    expect(loadPersistedStatus(NOW_MS)).toEqual({
      contextUsage,
      rateLimits: limits,
      observedAt: {
        contextUsage: { 'sess-1': NOW_MS, 'sess-2': NOW_MS },
        rateLimits: { claude: NOW_MS },
      },
    });
  });

  it('round-trips each provider\'s windows separately', () => {
    // Two accounts persisted side by side must come back side by side: a
    // restore that merged them would show one provider's limits under the
    // other's session, which is the whole thing the provider key prevents.
    const limits = rateLimits({
      codex: [window(SEVEN_DAYS, 44, NOW_SECONDS + 86400)],
    });

    savePersistedStatus(cached({}, limits));

    expect(loadPersistedStatus(NOW_MS).rateLimits).toEqual(limits);
  });

  it('restores a rate-limit window whose reset is still ahead regardless of snapshot age', () => {
    // The point of dropping the old snapshot-wide TTL: opening Delta the
    // morning after an evening session must still show the 7d row. A window
    // that has not reset yet carries a meaningful percentage — a lower bound,
    // since other devices can only have added to it — so its age is
    // irrelevant; only its own `resets_at` decides.
    const stillAhead = window(SEVEN_DAYS, 41, NOW_SECONDS + 3 * 86400);
    savePersistedStatus(cached({}, { claude: [stillAhead] }));

    // Read half a day later — many times the hour the old guard allowed.
    const result = loadPersistedStatus(NOW_MS + 12 * HOUR_MS);

    expect(result.rateLimits).toEqual({ claude: [stillAhead] });
    expect(result.observedAt.rateLimits).toEqual({ claude: NOW_MS });
  });

  it('drops a rate-limit window whose reset time has passed, keeping valid ones', () => {
    const stale = window(FIVE_HOURS, 90, NOW_SECONDS - 60);
    const live = window(SEVEN_DAYS, 8, NOW_SECONDS + 86400);
    // The 5h window already rolled over (its reset is in the past), so neither
    // its percentage nor its next reset instant is known and it must be
    // dropped rather than rendered as 0%; the 7d one survives.
    savePersistedStatus(cached({}, { claude: [stale, live] }));

    const result = loadPersistedStatus(NOW_MS);
    expect(result.rateLimits).toEqual({ claude: [live] });
  });

  it('leaves a provider with no live windows rather than dropping it', () => {
    savePersistedStatus(
      cached(
        { 'sess-1': 62 },
        {
          claude: [
            window(FIVE_HOURS, 90, NOW_SECONDS - 60),
            window(SEVEN_DAYS, 8, NOW_SECONDS - 120),
          ],
        },
      ),
    );

    const result = loadPersistedStatus(NOW_MS);
    // Context usage still restores; only the rolled-over windows are gone. An
    // empty list and an absent provider render identically (no rows), so there
    // is nothing to gain from collapsing one into the other.
    expect(result.contextUsage).toEqual({ 'sess-1': 62 });
    expect(result.rateLimits).toEqual({ claude: [] });
  });

  it('keeps a window with no reset time within its own fallback bound', () => {
    // Nothing to expire against, so the window is trusted for an hour measured
    // from ITS OWN observation, not from whenever the payload was last written.
    const undated = window(FIVE_HOURS, 50, null);
    savePersistedStatus(cached({}, { claude: [undated] }));

    expect(
      loadPersistedStatus(NOW_MS + RATE_LIMIT_FRESHNESS_MS).rateLimits,
    ).toEqual({ claude: [undated] });
  });

  it('drops a window with no reset time past its own fallback bound', () => {
    savePersistedStatus(cached({}, { claude: [window(FIVE_HOURS, 50, null)] }));

    expect(
      loadPersistedStatus(NOW_MS + RATE_LIMIT_FRESHNESS_MS + 1).rateLimits,
    ).toEqual({ claude: [] });
  });

  it('judges an undated window against its own observation, not a later save', () => {
    // What the two cases above claim ("from ITS OWN observation, not from
    // whenever the payload was last written") but cannot show on their own,
    // since nothing writes the payload a second time there. Claude goes quiet
    // holding an undated window while Codex keeps reporting: a save driven by
    // the Codex account must carry Claude's stamp forward untouched, or the
    // fallback bound would restart on every one of the OTHER account's frames
    // and the undated window would never expire.
    const undated = window(FIVE_HOURS, 50, null);
    savePersistedStatus(cached({}, { claude: [undated] }));

    const laterSave = NOW_MS + 30 * 60 * 1000;
    savePersistedStatus({
      contextUsage: {},
      rateLimits: {
        claude: [undated],
        codex: [window(SEVEN_DAYS, 44, NOW_SECONDS + 86400)],
      },
      observedAt: {
        contextUsage: {},
        rateLimits: { claude: NOW_MS, codex: laterSave },
      },
    });

    // Read just past the bound measured from CLAUDE's observation — still well
    // within it if measured from the Codex save.
    const result = loadPersistedStatus(NOW_MS + RATE_LIMIT_FRESHNESS_MS + 1);
    expect(result.rateLimits.claude).toEqual([]);
    expect(result.rateLimits.codex).toHaveLength(1);
    expect(result.observedAt.rateLimits).toEqual({
      claude: NOW_MS,
      codex: laterSave,
    });
  });

  it('restores context usage long after the hour the old guard allowed', () => {
    // An untouched session's context usage cannot have changed overnight: the
    // session emits no snapshot precisely because no agent is running in it.
    savePersistedStatus(cached({ 'sess-1': 62 }, {}));

    const result = loadPersistedStatus(NOW_MS + 20 * HOUR_MS);
    expect(result.contextUsage).toEqual({ 'sess-1': 62 });
    expect(result.observedAt.contextUsage).toEqual({ 'sess-1': NOW_MS });
  });

  it('drops a context-usage entry past the garbage-collection horizon', () => {
    savePersistedStatus(cached({ 'sess-1': 62 }, {}));

    // The bound is garbage collection, not freshness: without it the
    // session-keyed map would accumulate dead sessions forever.
    expect(
      loadPersistedStatus(NOW_MS + CONTEXT_USAGE_GC_TTL_MS).contextUsage,
    ).toEqual({ 'sess-1': 62 });
    expect(
      loadPersistedStatus(NOW_MS + CONTEXT_USAGE_GC_TTL_MS + 1).contextUsage,
    ).toEqual({});
  });

  it('carries each entry\'s own observed time through a save that refreshed another key', () => {
    // Three sessions last seen at very different times, as a long-lived
    // workspace produces.
    const ancient = NOW_MS - 40 * DAY_MS;
    const older = NOW_MS - 10 * DAY_MS;
    const observedAt = {
      contextUsage: {
        'sess-ancient': ancient,
        'sess-older': older,
        'sess-new': older,
      },
      rateLimits: {},
    };
    savePersistedStatus({
      contextUsage: { 'sess-ancient': 12, 'sess-older': 30, 'sess-new': 62 },
      rateLimits: {},
      observedAt,
    });
    // A live snapshot for `sess-new` only: its stamp moves to now and every
    // other entry keeps the one it had. A single snapshot-wide stamp would
    // have silently refreshed all three.
    savePersistedStatus({
      contextUsage: { 'sess-ancient': 12, 'sess-older': 30, 'sess-new': 70 },
      rateLimits: {},
      observedAt: {
        ...observedAt,
        contextUsage: { ...observedAt.contextUsage, 'sess-new': NOW_MS },
      },
    });

    const result = loadPersistedStatus(NOW_MS);
    // `sess-older` came back with its own ten-day-old stamp intact, not with
    // the time of the save that only touched `sess-new`…
    expect(result.observedAt.contextUsage).toEqual({
      'sess-older': older,
      'sess-new': NOW_MS,
    });
    // …and that carried-forward stamp is load-bearing: `sess-ancient` is past
    // the GC horizon on its own time while the other two survive.
    expect(result.contextUsage).toEqual({ 'sess-older': 30, 'sess-new': 70 });
  });

  it('skips an entry whose observation time is unknown rather than guessing one', () => {
    // The store always stamps what it writes; a value with no stamp could not
    // be judged by the per-datum rules on the way back in, so it is not
    // written at all.
    savePersistedStatus({
      contextUsage: { 'sess-1': 62 },
      rateLimits: rateLimits(),
      observedAt: { contextUsage: {}, rateLimits: {} },
    });

    expect(loadPersistedStatus(NOW_MS)).toEqual(EMPTY);
  });

  it('returns the empty result when nothing is persisted', () => {
    expect(loadPersistedStatus(NOW_MS)).toEqual(EMPTY);
  });

  it('returns the empty result (without throwing) for garbage in localStorage', () => {
    localStorage.setItem(STORAGE_KEY, 'not json at all {');

    expect(() => loadPersistedStatus(NOW_MS)).not.toThrow();
    expect(loadPersistedStatus(NOW_MS)).toEqual(EMPTY);
  });

  it('returns the empty result for the previous release\'s persisted shape', () => {
    // Every user upgrading into this change has yesterday's payload sitting in
    // localStorage: one snapshot-wide `savedAt`, bare percentages, and bare
    // window lists. It carries no version, so it is DETECTED and discarded
    // whole — never half-parsed into entries with NaN observation times, which
    // the expiry rules would then compare against.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        savedAt: NOW_MS,
        contextUsage: { 'sess-1': 62 },
        rateLimits: {
          claude: [window(FIVE_HOURS, 37, NOW_SECONDS + 3600)],
        },
      }),
    );

    expect(() => loadPersistedStatus(NOW_MS)).not.toThrow();
    expect(loadPersistedStatus(NOW_MS)).toEqual(EMPTY);
  });

  it('returns the empty result for a payload from an unknown future version', () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ version: 99, contextUsage: {}, rateLimits: {} }),
    );

    expect(loadPersistedStatus(NOW_MS)).toEqual(EMPTY);
  });

  it('drops an individual entry whose observation stamp is not a number', () => {
    // A hand-edited or truncated payload: the version matches but one entry is
    // unusable. It is skipped; the well-formed neighbours still restore.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        version: 2,
        contextUsage: {
          'sess-bad': { percentage: 12, observedAt: 'yesterday' },
          'sess-good': { percentage: 62, observedAt: NOW_MS },
        },
        rateLimits: {
          claude: { windows: [window(FIVE_HOURS, 37, NOW_SECONDS + 3600)] },
        },
      }),
    );

    const result = loadPersistedStatus(NOW_MS);
    expect(result.contextUsage).toEqual({ 'sess-good': 62 });
    expect(result.observedAt.contextUsage).toEqual({ 'sess-good': NOW_MS });
    expect(result.rateLimits).toEqual({});
  });
});
