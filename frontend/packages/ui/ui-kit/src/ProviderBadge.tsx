import { Badge } from './Badge';
import { cn } from './cn';

/**
 * The AI-agent providers a session can run on. Kept as a local string union so
 * ui-kit stays domain-agnostic (it must not depend on the wire/gateway layer);
 * the values match the wire `AgentProvider` type (`"claude" | "codex"`), so a
 * `session.provider` is assignable directly.
 */
export type Provider = 'claude' | 'codex';

interface ProviderMeta {
  /** Two-character monogram — the primary, color-independent distinguisher. */
  monogram: string;
  /** Full product name, surfaced as the tooltip and accessible name. */
  displayName: string;
  /**
   * Tailwind classes pairing the provider's foreground hue with a low-alpha
   * wash of the same token (mirrors Badge's soft `info`/`warning` tones).
   */
  className: string;
}

const PROVIDERS: Record<Provider, ProviderMeta> = {
  claude: {
    monogram: 'CL',
    displayName: 'Claude Code',
    className: 'bg-provider-claude/15 text-provider-claude',
  },
  codex: {
    monogram: 'CX',
    displayName: 'Codex',
    className: 'bg-provider-codex/15 text-provider-codex',
  },
};

export interface ProviderBadgeProps {
  provider: Provider;
  className?: string;
}

/**
 * A compact monogram chip identifying a session's AI-agent provider: `CL` for
 * Claude Code, `CX` for Codex, each in the provider's accent hue. The two
 * letters distinguish the providers on their own; the color is a redundant
 * reinforcement (so the chip still reads under color-vision differences). The
 * full product name is the tooltip and the accessible name.
 *
 * Built on {@link Badge} and living in ui-kit so every feature can share one
 * definition. For a dense row, where the colored chip would shout, reach for
 * the quiet {@link ProviderIcon} instead.
 */
export function ProviderBadge({ provider, className }: ProviderBadgeProps) {
  const meta = PROVIDERS[provider];
  return (
    <Badge
      className={cn(meta.className, className)}
      title={meta.displayName}
      aria-label={meta.displayName}
    >
      {/* aria-hidden: the monogram is decorative here — the accessible name is
          carried by the Badge's aria-label above so screen readers announce the
          full product name, not "CL"/"CX". */}
      <span aria-hidden>{meta.monogram}</span>
    </Badge>
  );
}
