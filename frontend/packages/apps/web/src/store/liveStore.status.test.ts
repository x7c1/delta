/**
 * `status_updated` snapshots, restored status provenance, and status
 * persistence.
 *
 * Shared helpers live in liveStore.testkit.ts.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import { useLiveStore } from './liveStore';
import {
  RATE_LIMIT_FRESHNESS_MS,
  loadPersistedStatus,
  staleRateLimitsObservedAt,
} from './statusPersistence';
import {
  FIVE_HOURS,
  reset,
  SEVEN_DAYS,
  statusSnapshot,
  window,
} from './liveStore.testkit';

describe('liveStore status_updated', () => {
  beforeEach(reset);

  it('keeps only the latest per-session context usage (replace, not append)', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 40 }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 55 }),
    });
    // Only the most recent snapshot is retained for the session.
    expect(useLiveStore.getState().contextUsage).toEqual({ 'sess-1': 55 });
  });

  it('keeps context usage keyed per session (one session does not clobber another)', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 40 }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-2',
      snapshot: statusSnapshot({ context_used_percentage: 70 }),
    });
    // Each session retains its own latest value; the second event does not
    // overwrite the first (context is per session, unlike the global rate limit).
    expect(useLiveStore.getState().contextUsage).toEqual({
      'sess-1': 40,
      'sess-2': 70,
    });
  });

  it('keeps one rate-limit snapshot per provider, replaced by the latest', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({
        rate_limits: [window(FIVE_HOURS, 10, 100)],
      }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-2',
      snapshot: statusSnapshot({
        rate_limits: [window(FIVE_HOURS, 30, 300)],
      }),
    });
    // Both sessions are Claude sessions reporting the same account, so the
    // later event simply replaces that provider's windows.
    expect(useLiveStore.getState().rateLimits).toEqual({
      claude: [window(FIVE_HOURS, 30, 300)],
    });
  });

  it('keeps each provider\'s rate limits apart', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-claude',
      snapshot: statusSnapshot({
        rate_limits: [window(FIVE_HOURS, 10, 100)],
      }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        rate_limits: [window(SEVEN_DAYS, 55, 500)],
      }),
    });
    // A Codex account update must not overwrite the Claude account's windows:
    // they are different accounts that happen to share a footer.
    expect(useLiveStore.getState().rateLimits).toEqual({
      claude: [window(FIVE_HOURS, 10, 100)],
      codex: [window(SEVEN_DAYS, 55, 500)],
    });
  });

  it('leaves rate limits alone when a snapshot says nothing about them', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        rate_limits: [window(SEVEN_DAYS, 55, 500)],
      }),
    });
    // A token-usage frame carries `rate_limits: null` — no statement — and the
    // account's windows must survive it. (A provider that reports usage and
    // limits on separate frames would otherwise wipe its own footer every turn.)
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        context_used_percentage: 25,
      }),
    });
    expect(useLiveStore.getState().rateLimits).toEqual({
      codex: [window(SEVEN_DAYS, 55, 500)],
    });
    expect(useLiveStore.getState().contextUsage).toEqual({ 'sess-codex': 25 });
  });

  it('clears a provider\'s windows when it reports it has none', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ rate_limits: [window(FIVE_HOURS, 10, 100)] }),
    });
    // An empty list IS a statement — "this account has no windows" — so the
    // rows clear (e.g. a subscription that lapsed), unlike a `null`.
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ rate_limits: [] }),
    });
    expect(useLiveStore.getState().rateLimits).toEqual({ claude: [] });
  });

  it('drops a session context entry when the percentage goes null (e.g. /compact)', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 40 }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: null }),
    });
    expect(useLiveStore.getState().contextUsage).toEqual({});
  });
});

describe('liveStore restored status provenance', () => {
  beforeEach(reset);

  /** Roughly "last night", as an overnight reload would restore. */
  const OBSERVED_AT = Date.now() - 9 * 60 * 60 * 1000;

  /** The state a reload leaves behind: values restored, none confirmed live. */
  function seedRestored() {
    useLiveStore.setState({
      contextUsage: { 'sess-1': 62 },
      rateLimits: {
        claude: [window(FIVE_HOURS, 30, 100)],
        codex: [window(SEVEN_DAYS, 55, 500)],
      },
      statusObservedAt: {
        contextUsage: { 'sess-1': OBSERVED_AT },
        rateLimits: { claude: OBSERVED_AT, codex: OBSERVED_AT },
      },
      restoredRateLimitsObservedAt: { claude: OBSERVED_AT, codex: OBSERVED_AT },
    });
  }

  // Which providers the seed marks is decided by `staleRateLimitsObservedAt`,
  // and this exercises it there. The seed line itself runs once at module load
  // (`createStatusSlice` reads the persisted snapshot at import time), so it is
  // not reachable from a test that shares the store instance — the same reason
  // `liveStore status persistence` below drives `loadPersistedStatus` directly
  // rather than re-importing the store. The seed applies this function and
  // nothing else.
  it('seeds the stale mark only for providers observed past the freshness bound', () => {
    const now = Date.now();
    const overnight = now - 9 * 60 * 60 * 1000;
    const justNow = now - 60 * 1000;

    // Claude went quiet last night, so its percentage is a lower bound of
    // unknown age and saying so is the point. Codex reported a minute ago and
    // the user merely reloaded the tab: that reading is still exactly right,
    // and dimming it would be a claim nothing could take back — the server
    // replays no status on connect, so the mark would sit there until the
    // user's next agent turn.
    expect(
      staleRateLimitsObservedAt({ claude: overnight, codex: justNow }, now),
    ).toEqual({ claude: overnight });

    // The bound itself still counts as fresh, exactly as the undated-window
    // fallback in `statusPersistence` reads the same constant.
    expect(
      staleRateLimitsObservedAt({ claude: now - RATE_LIMIT_FRESHNESS_MS }, now),
    ).toEqual({});
    expect(
      staleRateLimitsObservedAt(
        { claude: now - RATE_LIMIT_FRESHNESS_MS - 1 },
        now,
      ),
    ).toEqual({ claude: now - RATE_LIMIT_FRESHNESS_MS - 1 });
  });

  it('marks restored status stale until live data arrives for that key', () => {
    seedRestored();

    // The first live snapshot that STATES rate limits clears its own
    // provider's mark and nobody else's: the Claude account speaking says
    // nothing about the Codex one. It arrives on a different session than the
    // restored context entry, which therefore stays untouched below.
    useLiveStore.getState().applyEvent({
      kind: 'status_updated',
      session_id: 'sess-claude',
      snapshot: statusSnapshot({ rate_limits: [window(FIVE_HOURS, 31, 100)] }),
    });

    expect(useLiveStore.getState().restoredRateLimitsObservedAt).toEqual({
      codex: OBSERVED_AT,
    });
    // Context usage is deliberately never marked stale, so there is no
    // per-session provenance map at all: a restored percentage belongs to a
    // session, and a session emitting no snapshot has no agent running that
    // could change it — a mark there would never be cleared and would sit
    // greyed out indefinitely.
    expect('restoredContextUsageObservedAt' in useLiveStore.getState()).toBe(
      false,
    );
    expect(useLiveStore.getState().contextUsage).toEqual({ 'sess-1': 62 });
  });

  it('keeps a provider\'s stale mark through a token-usage-only frame', () => {
    seedRestored();

    // Codex reports usage and account limits on separate frames. A usage frame
    // carries `rate_limits: null` — no statement about the account — so it must
    // not vouch for the restored windows any more than it may overwrite them.
    useLiveStore.getState().applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        context_used_percentage: 25,
      }),
    });

    expect(useLiveStore.getState().restoredRateLimitsObservedAt).toEqual({
      claude: OBSERVED_AT,
      codex: OBSERVED_AT,
    });
  });

  it('stamps the observation time of what a snapshot stated, and only that', () => {
    seedRestored();

    useLiveStore.getState().applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        context_used_percentage: 25,
      }),
    });

    const observed = useLiveStore.getState().statusObservedAt;
    // The session it spoke for is stamped now; the one it did not mention, and
    // both accounts' windows, keep the times they were restored with. That is
    // what lets each datum expire on its own terms after the next reload.
    expect(observed.contextUsage['sess-codex']).toBeGreaterThan(OBSERVED_AT);
    expect(observed.contextUsage['sess-1']).toBe(OBSERVED_AT);
    expect(observed.rateLimits).toEqual({
      claude: OBSERVED_AT,
      codex: OBSERVED_AT,
    });
  });

  it('forgets a session\'s observation time when its percentage goes null', () => {
    seedRestored();

    // `/compact` drops the entry; leaving its stamp behind would persist a
    // dangling observation for a value that no longer exists.
    useLiveStore.getState().applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: null }),
    });

    expect(useLiveStore.getState().statusObservedAt.contextUsage).toEqual({});
  });
});

describe('liveStore status persistence', () => {
  beforeEach(() => {
    reset();
    localStorage.clear();
  });

  it('persists the latest status snapshot on status_updated so a reload can restore it', () => {
    useLiveStore.getState().applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({
        context_used_percentage: 62,
        // Reset times in the far future so the freshness pruning keeps them
        // (resets_at is epoch seconds; load compares against now/1000).
        rate_limits: [
          window(FIVE_HOURS, 30, Date.now() / 1000 + 3600),
          window(SEVEN_DAYS, 9, Date.now() / 1000 + 5 * 86400),
        ],
      }),
    });

    // What a fresh page load would read back at init (the store seeds
    // contextUsage / rateLimits from this on startup).
    const restored = loadPersistedStatus(Date.now());
    expect(restored.contextUsage).toEqual({ 'sess-1': 62 });
    expect(restored.rateLimits).toEqual({
      claude: [
        { ...window(FIVE_HOURS, 30, expect.any(Number)) },
        { ...window(SEVEN_DAYS, 9, expect.any(Number)) },
      ],
    });
  });

  it('round-trips a multi-provider snapshot through localStorage', () => {
    const store = useLiveStore.getState();
    const resetsAt = Date.now() / 1000 + 3600;
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-claude',
      snapshot: statusSnapshot({ rate_limits: [window(FIVE_HOURS, 30, resetsAt)] }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        context_used_percentage: 12,
        rate_limits: [window(SEVEN_DAYS, 44, resetsAt)],
      }),
    });

    // Both accounts survive the reload, still apart: a restore that collapsed
    // them would resurrect exactly the cross-provider leak the keying prevents.
    const restored = loadPersistedStatus(Date.now());
    expect(restored.rateLimits).toEqual({
      claude: [window(FIVE_HOURS, 30, resetsAt)],
      codex: [window(SEVEN_DAYS, 44, resetsAt)],
    });
    expect(restored.contextUsage).toEqual({ 'sess-codex': 12 });
  });
});
