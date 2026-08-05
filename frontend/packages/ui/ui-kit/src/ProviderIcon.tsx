import type { CSSProperties } from 'react';
// Brand marks from `@lobehub/icons-static-svg` (MIT), pinned to an exact
// version so the pin plus the lockfile integrity hash decides which bytes get
// installed. Imported rather than copied into this repository, so the licensing
// provenance stays with the dependency.
import claudeMark from '@lobehub/icons-static-svg/icons/claude.svg';
import codexMark from '@lobehub/icons-static-svg/icons/codex.svg';
import { cn } from './cn';
import { PROVIDER_DISPLAY_NAMES, type Provider } from './provider';

/** URL of each provider's brand-mark SVG, used as the element's CSS mask. */
const MARKS: Record<Provider, string> = {
  claude: claudeMark,
  codex: codexMark,
};

/**
 * The mask declarations that paint one brand mark in the inherited text color.
 * `-webkit-` twins are required by WebKitGTK (the desktop shell's engine),
 * which still ships the prefixed properties.
 */
function maskStyle(url: string): CSSProperties {
  // Double-quoted on purpose. The bundler inlines a small SVG as a
  // `data:image/svg+xml,…` URI that still carries the markup's single quotes
  // (`fill='currentColor'`), and a bare quote inside an unquoted `url()` token
  // makes the whole declaration invalid — the browser then drops the mask and
  // paints a solid `currentColor` square. The URL itself never contains a
  // double quote, so wrapping it in double quotes is always safe.
  const image = `url("${url}")`;
  return {
    maskImage: image,
    WebkitMaskImage: image,
    maskRepeat: 'no-repeat',
    WebkitMaskRepeat: 'no-repeat',
    maskPosition: 'center',
    WebkitMaskPosition: 'center',
    maskSize: 'contain',
    WebkitMaskSize: 'contain',
  };
}

export interface ProviderIconProps {
  provider: Provider;
  className?: string;
}

/**
 * A small monochrome brand mark identifying a session's AI-agent provider: the
 * Claude spark for Claude Code, the Codex mark for Codex. Sized to `1em` and
 * painted in `currentColor`, it takes the font size and color of whatever line
 * it sits on. That makes it the quiet counterpart to {@link ProviderDot},
 * for dense rows (the navigator session card's meta line) where a colored chip
 * would shout. The full product name is the tooltip and the accessible name.
 *
 * The mark is painted as a CSS mask over `bg-current` rather than as an
 * `<img>`, which cannot inherit `currentColor`.
 */
export function ProviderIcon({ provider, className }: ProviderIconProps) {
  const name = PROVIDER_DISPLAY_NAMES[provider];
  return (
    <span
      role="img"
      // translate-y: a box with no text synthesizes its baseline at its
      // bottom edge, so in a baseline-aligned row the full 1em square would
      // sit entirely above the baseline — a head taller than neighboring
      // digits (cap height ≈ 0.7em) and optically ~2px high. Nudging down
      // 0.125em recenters the mark on the text's optical middle (the same
      // correction icon fonts apply via `vertical-align: -0.125em`); a
      // transform also works where vertical-align does not (flex items).
      className={cn(
        'inline-flex h-[1em] w-[1em] shrink-0 translate-y-[0.125em]',
        className,
      )}
      title={name}
      aria-label={name}
    >
      {/* aria-hidden: the glyph is decorative — the accessible name is carried
          by the wrapper's aria-label above. The mask cuts the mark out of
          `bg-current`, which is how the icon inherits the surrounding text
          color, so it must never be paired with a provider accent utility. */}
      <span
        aria-hidden
        className="h-full w-full bg-current"
        style={maskStyle(MARKS[provider])}
      />
    </span>
  );
}
