import type { AgentProvider, RateLimitWindow } from '@delta/wire-gen';

/**
 * The rate-limit windows most recently observed for one provider's account,
 * most significant first. Empty when the account reports none (a non-Pro/Max
 * Claude account, or before the first API response of the day), in which case
 * the footer shows no rows at all rather than empty bars.
 *
 * A window carries its own duration, so the display is driven by the data
 * rather than by a hardcoded 5h/7d pair — see `RateLimitWindow.duration_seconds`.
 */
export type RateLimitWindows = RateLimitWindow[];

/**
 * Rate limits keyed by provider.
 *
 * Rate limits are scoped to an **account**, which means to a provider: every
 * Claude session reports the same Claude windows, every Codex session the same
 * Codex ones, and the two have nothing to do with each other. A single global
 * slot would therefore show whichever provider spoke last — including showing
 * Claude's limits while a Codex session is focused, which reads as a statement
 * about Codex and is simply false. Keying by provider makes that impossible:
 * the footer looks up the focused session's provider and can only ever find
 * that provider's numbers.
 *
 * Within a provider it is last-writer-wins: the latest snapshot for a provider
 * replaces its windows. A provider with no entry has never reported limits.
 *
 * This shape lives here rather than in the store so the persistence layer can
 * reference it without importing the store (which itself imports the
 * persistence layer — a cycle the linter rejects).
 */
export type RateLimitsByProvider = Partial<Record<AgentProvider, RateLimitWindows>>;

/**
 * Epoch-ms instants at which each provider's rate-limit windows were last
 * observed, keyed the same way {@link RateLimitsByProvider} is.
 *
 * Used for two distinct jobs: the persistence layer expires each provider's
 * windows against their OWN observation (see `statusPersistence`), and the
 * footer dates the rows it restored from `localStorage` but has not yet seen
 * confirmed live.
 */
export type RateLimitsObservedAt = Partial<Record<AgentProvider, number>>;

/**
 * When each piece of the status snapshot was observed (epoch ms), keyed
 * exactly as the values themselves are: per session for context usage, per
 * provider for rate limits.
 *
 * Per key, not per snapshot: every datum here expires on its own terms — a
 * rate-limit window against its `resets_at` (or, lacking one, against its own
 * observation), a context-usage entry against a long garbage-collection
 * horizon — so a single "when was this written" stamp would let a session or
 * provider that went quiet days ago inherit the freshness of whichever key
 * happened to speak last.
 */
export interface StatusObservedAt {
  contextUsage: Record<string, number>;
  rateLimits: RateLimitsObservedAt;
}
