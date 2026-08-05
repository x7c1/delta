import { cn } from './cn';
import { PROVIDER_DISPLAY_NAMES, type Provider } from './provider';

const NAME_CLASSES: Record<Provider, string> = {
  claude: 'text-provider-claude',
  codex: 'text-provider-codex',
};

export interface ProviderNameProps {
  provider: Provider;
  className?: string;
}

/**
 * The provider's full product name written in its accent hue: "Claude Code"
 * in burnt orange, "Codex" in green — the same hue channel the navigator
 * session card speaks through its kebab trigger, so every surface that names
 * a provider reinforces one vocabulary: provider = hue. Deliberately
 * shape-free: a small filled dot would share the visual language of
 * {@link StatusDot}'s open/closed indicator, and a green provider dot next
 * to a green "open" dot invites misreading. Coloring the written name adds
 * no glyph at all, and the words themselves keep the marker readable without
 * relying on color.
 */
export function ProviderName({ provider, className }: ProviderNameProps) {
  return (
    <span className={cn(NAME_CLASSES[provider], className)}>
      {PROVIDER_DISPLAY_NAMES[provider]}
    </span>
  );
}
