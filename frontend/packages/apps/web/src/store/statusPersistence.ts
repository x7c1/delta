import type { AgentProvider, RateLimitWindow } from '@delta/wire-gen';
import type {
  RateLimitsByProvider,
  RateLimitsObservedAt,
  RateLimitWindows,
  StatusObservedAt,
} from './statusTypes';

/**
 * Best-effort persistence of the usage snapshot (per-session context usage +
 * per-provider account rate limits) across page reloads.
 *
 * The snapshot is ephemeral: the server only broadcasts it live and the store
 * resets on reload, so without this the context bar and rate-limit footer go
 * blank until the next usage event fires. We keep the last one in
 * `localStorage` and restore it on load.
 *
 * Freshness is decided **per datum**, never per snapshot. A blanket
 * wall-clock TTL over the whole payload is the wrong guard: it blanks the
 * footer and the context bar exactly when they are most wanted — the morning
 * after an evening session, when "how much of the 7d window is left?" is the
 * first question asked. Instead:
 *
 * - A rate-limit window carries its own expiry, so it is judged by
 *   `resets_at`: still ahead → restored no matter how old the snapshot is
 *   (the percentage is a lower bound — other devices can only have added to
 *   it); already passed → dropped, because after a rollover neither the new
 *   percentage nor the next reset instant is known. A window with no
 *   `resets_at` has nothing to expire against, so it falls back to
 *   {@link RATE_LIMIT_FRESHNESS_MS} measured against its own observation.
 * - A context-usage percentage belongs to a session, and a session emitting no
 *   live snapshot has no agent running to change it — so age says nothing
 *   about correctness and the entry is restored regardless. The only pruning
 *   is garbage collection: {@link CONTEXT_USAGE_GC_TTL_MS} keeps the
 *   session-keyed map from accumulating dead sessions forever.
 *
 * Both rules need to know when each datum was observed, so the payload carries
 * an observation time per session and per provider ({@link StatusObservedAt})
 * rather than one stamp for the whole snapshot. A save carries every entry's
 * own time forward untouched; only a live update for that key refreshes it.
 *
 * Only this small, non-sensitive slice is persisted — never sessions, threads,
 * or message content, which come from the server.
 */

const STORAGE_KEY = 'delta:status-snapshot';

/**
 * Layout version of the stored payload. A payload carrying anything else —
 * including nothing at all, which is what every pre-versioned release wrote —
 * is discarded wholesale rather than half-parsed into missing values and NaN
 * timestamps. This is a best-effort cache, so one blank restore on upgrade is
 * an acceptable price for never reasoning about an older shape again.
 */
const SCHEMA_VERSION = 2;

/**
 * How long a rate-limit observation still counts as current without any live
 * confirmation. One bound, two consumers:
 *
 * - A window whose `resets_at` is `null` has no reset instant to be judged
 *   against, so it is trusted for this long past its own observation and
 *   dropped after that.
 * - A restored provider older than this is the one the footer marks stale
 *   (see {@link staleRateLimitsObservedAt}); a fresher one renders as live.
 *
 * The two are the same question asked twice — "is this reading still worth
 * presenting as the current state of the account?" — so they share the bound
 * rather than each picking their own hour.
 */
export const RATE_LIMIT_FRESHNESS_MS = 60 * 60 * 1000; // 1 hour

/**
 * The providers in `observedAt` whose readings are past
 * {@link RATE_LIMIT_FRESHNESS_MS}, mapped to when they were observed — what the
 * store seeds its restored-provenance map with on load.
 *
 * Only the stale ones, never every restored provider: a reload a minute after
 * the last turn restores a reading that is still exactly right, and dimming it
 * would tell the user it is old when it is not. Worse, nothing would undim it —
 * the server replays no status on connect, so the mark would sit there until
 * the user's next agent turn. Past the bound the opposite is true: an overnight
 * reading really is a lower bound of unknown age, and saying so is the point.
 */
export function staleRateLimitsObservedAt(
  observedAt: RateLimitsObservedAt,
  now: number,
): RateLimitsObservedAt {
  const stale: RateLimitsObservedAt = {};
  for (const [key, at] of Object.entries(observedAt)) {
    if (at !== undefined && now - at > RATE_LIMIT_FRESHNESS_MS) {
      stale[key as AgentProvider] = at;
    }
  }
  return stale;
}

/**
 * Garbage-collection horizon for context usage. Not a freshness guard — a
 * restored percentage stays correct for as long as its session sits idle — but
 * the map is keyed by session id, so without an upper bound every session ever
 * focused would keep an entry forever.
 */
export const CONTEXT_USAGE_GC_TTL_MS = 30 * 24 * 60 * 60 * 1000; // 30 days

/**
 * The status slice this module caches, in both directions: the values plus,
 * per key, when each was observed. `loadPersistedStatus` returns one and
 * `savePersistedStatus` takes one, so the observation times survive a reload
 * → save → reload cycle unchanged.
 */
export interface CachedStatus {
  contextUsage: Record<string, number>;
  rateLimits: RateLimitsByProvider;
  observedAt: StatusObservedAt;
}

/** One session's persisted context-usage reading. */
interface PersistedContextUsage {
  percentage: number;
  observedAt: number;
}

/** One provider's persisted rate-limit windows. */
interface PersistedRateLimits {
  windows: RateLimitWindows;
  observedAt: number;
}

/** The `delta:status-snapshot` payload, as written by {@link savePersistedStatus}. */
interface PersistedStatus {
  version: number;
  contextUsage: Record<string, PersistedContextUsage>;
  rateLimits: Partial<Record<AgentProvider, PersistedRateLimits>>;
}

/**
 * The same payload on the way back in, after the version matched and both maps
 * turned out to be objects. Their entries stay `unknown`: a truncated or
 * hand-edited payload can carry anything under a matching version, so it is
 * {@link restore} — not this shape — that vouches for each one.
 */
interface ParsedStatus {
  contextUsage: Record<string, unknown>;
  rateLimits: Record<string, unknown>;
}

function emptyStatus(): CachedStatus {
  return {
    contextUsage: {},
    rateLimits: {},
    observedAt: { contextUsage: {}, rateLimits: {} },
  };
}

export function loadPersistedStatus(now: number): CachedStatus {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) {
      return emptyStatus();
    }
    const payload = asParsedStatus(JSON.parse(raw));
    return payload === null ? emptyStatus() : restore(payload, now);
  } catch {
    // Unreadable localStorage or malformed JSON — the cache is optional.
    return emptyStatus();
  }
}

export function savePersistedStatus(status: CachedStatus): void {
  const payload: PersistedStatus = {
    version: SCHEMA_VERSION,
    contextUsage: {},
    rateLimits: {},
  };
  // Each value is written next to ITS OWN observation time, so a save driven
  // by one session's update leaves every other entry's stamp exactly as it
  // was. An entry whose observation time is unknown cannot be judged by the
  // per-datum rules on the way back in, so it is skipped rather than written
  // with a guessed stamp.
  for (const [sessionId, percentage] of Object.entries(status.contextUsage)) {
    const observedAt = status.observedAt.contextUsage[sessionId];
    if (observedAt !== undefined) {
      payload.contextUsage[sessionId] = { percentage, observedAt };
    }
  }
  for (const [key, windows] of Object.entries(status.rateLimits)) {
    const provider = key as AgentProvider;
    const observedAt = status.observedAt.rateLimits[provider];
    if (windows !== undefined && observedAt !== undefined) {
      payload.rateLimits[provider] = { windows, observedAt };
    }
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // localStorage unavailable (private mode, quota, SSR) — persistence is optional.
  }
}

/**
 * Narrow an unknown parsed payload to {@link ParsedStatus}, or `null` when it
 * is not one this release wrote. The version check is what makes an older
 * shape *detected* rather than silently half-read.
 */
function asParsedStatus(value: unknown): ParsedStatus | null {
  if (!isRecord(value) || value.version !== SCHEMA_VERSION) {
    return null;
  }
  const { contextUsage, rateLimits } = value;
  if (!isRecord(contextUsage) || !isRecord(rateLimits)) {
    return null;
  }
  return { contextUsage, rateLimits };
}

/**
 * Apply the per-datum rules described at the top of this module, keeping each
 * entry's own observation time so the store can persist it forward and date
 * the rows it has not yet seen confirmed live.
 */
function restore(payload: ParsedStatus, now: number): CachedStatus {
  const restored = emptyStatus();

  for (const [sessionId, entry] of Object.entries(payload.contextUsage)) {
    if (
      !isRecord(entry) ||
      !isFiniteNumber(entry.percentage) ||
      !isFiniteNumber(entry.observedAt) ||
      now - entry.observedAt > CONTEXT_USAGE_GC_TTL_MS
    ) {
      continue;
    }
    restored.contextUsage[sessionId] = entry.percentage;
    restored.observedAt.contextUsage[sessionId] = entry.observedAt;
  }

  for (const [key, entry] of Object.entries(payload.rateLimits)) {
    if (
      !isRecord(entry) ||
      !Array.isArray(entry.windows) ||
      !isFiniteNumber(entry.observedAt)
    ) {
      continue;
    }
    const observedAt = entry.observedAt;
    const provider = key as AgentProvider;
    // A provider left with no live window keeps an empty list rather than
    // being deleted: "this account's windows all rolled over" and "this
    // provider was never heard from" both render as no rows, and collapsing
    // them would only add a case to reason about.
    restored.rateLimits[provider] = entry.windows.filter(
      (window): window is RateLimitWindow =>
        isRecord(window) && isLiveWindow(window, observedAt, now),
    );
    restored.observedAt.rateLimits[provider] = observedAt;
  }

  return restored;
}

/**
 * Whether a persisted window still says something true. A window with a reset
 * instant is judged by it alone (its age is irrelevant — the percentage can
 * only have grown since); one without falls back to
 * {@link RATE_LIMIT_FRESHNESS_MS} against its own observation.
 */
function isLiveWindow(
  window: Record<string, unknown>,
  observedAt: number,
  now: number,
): boolean {
  const resetsAt = window.resets_at;
  if (isFiniteNumber(resetsAt)) {
    return resetsAt >= now / 1000;
  }
  return now - observedAt <= RATE_LIMIT_FRESHNESS_MS;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}
