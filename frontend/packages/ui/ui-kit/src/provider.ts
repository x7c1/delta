/**
 * The AI-agent providers a session can run on. Kept as a local string union so
 * ui-kit stays domain-agnostic (it must not depend on the wire/gateway layer);
 * the values match the wire `AgentProvider` type (`"claude" | "codex"`), so a
 * `session.provider` is assignable directly.
 */
export type Provider = 'claude' | 'codex';

/**
 * Full product name per provider, surfaced as the tooltip and accessible name
 * of every provider marker ({@link ProviderDot}, {@link ProviderIcon}).
 */
export const PROVIDER_DISPLAY_NAMES: Record<Provider, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
};
