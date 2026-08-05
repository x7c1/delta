import { cn } from './cn';
import { PROVIDER_DISPLAY_NAMES, type Provider } from './provider';

const DOT_CLASSES: Record<Provider, string> = {
  claude: 'bg-provider-claude',
  codex: 'bg-provider-codex',
};

export interface ProviderDotProps {
  provider: Provider;
  className?: string;
}

/**
 * A small round dot filled with a provider's accent hue: orange for Claude
 * Code, green for Codex. The hue is the same channel the navigator session
 * card speaks — its kebab trigger's dots rest in the provider color — so
 * every surface that names a provider (the new-session selector, Settings'
 * default-provider picker, the launch-option form and rows) reinforces one
 * vocabulary: provider = hue. The full product name is the tooltip and the
 * accessible name, so the marker never relies on color alone; pickers pair
 * the dot with the written product name anyway.
 */
export function ProviderDot({ provider, className }: ProviderDotProps) {
  const name = PROVIDER_DISPLAY_NAMES[provider];
  return (
    <span
      role="img"
      title={name}
      aria-label={name}
      className={cn(
        'inline-block h-2 w-2 shrink-0 rounded-full',
        DOT_CLASSES[provider],
        className,
      )}
    />
  );
}
