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
