import type { RateLimitsByProvider, RateLimitWindows } from './statusTypes';

/**
 * Best-effort persistence of the usage snapshot (per-session context usage +
 * per-provider account rate limits) across page reloads.
 *
 * The snapshot is ephemeral: the server only broadcasts it live and the store
 * resets on reload, so without this the context bar and rate-limit footer go
 * blank until the next usage event fires. We keep the last snapshot in
 * `localStorage` and restore it on load, with two freshness guards so a
 * long-idle restart never shows stale "mystery" values:
 *
 * - the whole snapshot is dropped if older than {@link TTL_MS}; and
 * - a rate-limit window whose reset time has already passed is dropped (its
 *   percentage is meaningless once the window rolled over).
 *
 * Only this small, non-sensitive slice is persisted — never sessions, threads,
 * or message content, which come from the server.
 */

const STORAGE_KEY = 'delta:status-snapshot';

/** Snapshots older than this are discarded on load (a long-idle restart starts blank). */
const TTL_MS = 60 * 60 * 1000; // 1 hour

export interface RestoredStatus {
  contextUsage: Record<string, number>;
  rateLimits: RateLimitsByProvider;
}

interface PersistedStatus extends RestoredStatus {
  /** Epoch ms when this snapshot was written. */
  savedAt: number;
}

const EMPTY: RestoredStatus = { contextUsage: {}, rateLimits: {} };

export function loadPersistedStatus(now: number): RestoredStatus {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return EMPTY;
    const parsed = JSON.parse(raw) as PersistedStatus;
    if (typeof parsed.savedAt !== 'number' || now - parsed.savedAt > TTL_MS) {
      return EMPTY;
    }
    return {
      contextUsage: parsed.contextUsage ?? {},
      rateLimits: dropExpiredWindows(parsed.rateLimits ?? {}, now),
    };
  } catch {
    return EMPTY;
  }
}

export function savePersistedStatus(
  contextUsage: Record<string, number>,
  rateLimits: RateLimitsByProvider,
  now: number,
): void {
  try {
    const payload: PersistedStatus = { savedAt: now, contextUsage, rateLimits };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // localStorage unavailable (private mode, quota, SSR) — persistence is optional.
  }
}

/**
 * Drop every rate-limit window whose reset time (epoch seconds) has already
 * passed, provider by provider. A provider left with no live window keeps an
 * empty list rather than being deleted: "this account's windows all rolled
 * over" and "this provider was never heard from" both render as no rows, and
 * collapsing them would only add a case to reason about.
 */
function dropExpiredWindows(
  rateLimits: RateLimitsByProvider,
  now: number,
): RateLimitsByProvider {
  const nowSeconds = now / 1000;
  const live = (windows: RateLimitWindows): RateLimitWindows =>
    windows.filter((w) => w.resets_at === null || w.resets_at >= nowSeconds);
  return Object.fromEntries(
    Object.entries(rateLimits).map(([provider, windows]) => [
      provider,
      live(windows ?? []),
    ]),
  );
}
