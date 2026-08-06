import type { AgentProvider } from '@delta/wire-gen';

/**
 * Display metadata for one AI-agent provider. Enumeration and naming only —
 * capability differences are deliberately NOT modelled here: those stay
 * server-driven (`GET /api/providers` and each session's capability profile),
 * so a UI never branches on the provider id for behavior.
 */
export interface ProviderMetadata {
  /**
   * The full product name, written in the provider hue by the shared
   * `ProviderName` wherever a picker or row names a provider.
   */
  label: string;
  /**
   * A one-line qualifier for pickers with room for a second line (Settings'
   * default-provider picker, the launch-option form).
   */
  hint: string;
}

/**
 * The single source of truth for the providers the web app enumerates: every
 * variant of the wire `AgentProvider` union, in display order, with its
 * display metadata. The `satisfies Record<AgentProvider, …>` clause makes the
 * compiler reject BOTH an unknown key and a missing one — when the wire union
 * gains a variant, this module fails to typecheck until the new provider gets
 * metadata, instead of the provider silently missing from selectors or being
 * dropped by persistence hydration.
 */
export const PROVIDER_METADATA = {
  claude: { label: 'Claude Code', hint: 'Anthropic Claude Code CLI' },
  codex: { label: 'Codex', hint: 'OpenAI Codex CLI' },
} as const satisfies Record<AgentProvider, ProviderMetadata>;

/**
 * Every {@link AgentProvider}, in display order. Derived from
 * {@link PROVIDER_METADATA}'s key order so the exhaustiveness guarantee
 * carries over: a provider that compiles there cannot be missing here. Used
 * by persistence hydration to validate a persisted provider value.
 */
export const AGENT_PROVIDERS = Object.keys(PROVIDER_METADATA) as readonly (
  keyof typeof PROVIDER_METADATA
)[];

/**
 * The providers as `{ value, label, hint }` options for the radio-group style
 * pickers (the new-session provider selector, Settings' default-provider
 * picker, the launch-option form), in display order.
 */
export const PROVIDER_OPTIONS: readonly {
  value: AgentProvider;
  label: string;
  hint: string;
}[] = AGENT_PROVIDERS.map((value) => ({ value, ...PROVIDER_METADATA[value] }));

/**
 * The provider the app defaults to wherever a provider choice needs a value
 * before the user has made one: the fresh-install default-provider setting,
 * and the new-session selection before the persisted setting seeds it.
 * Distinct from the wire-level omit
 * default (`PROVIDER_WIRE_DEFAULT` in `@delta/wire-gen`): the two coincide
 * today, but this one is a product choice that may move, while the wire
 * default is a fixed backend contract.
 */
export const DEFAULT_PROVIDER: AgentProvider = 'claude';
