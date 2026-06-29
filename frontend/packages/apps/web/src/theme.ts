/**
 * Runtime access to the app's design tokens.
 *
 * The token system has three layers, each defined exactly once:
 *
 * - **Values** — CSS custom properties on `:root` in `src/index.css`
 *   (overlay layout constants, the terminal background) plus the font stacks
 *   in `tailwind.config.js` (`theme.extend.fontFamily`), which the same
 *   `:root` block re-exposes as variables via `theme()`.
 * - **Utilities** — `tailwind.config.js` maps utility names onto the
 *   variables (`bg-terminal-bg`, `pb-composer-reserve`, `inset-x-overlay-inset`,
 *   …), so styled markup resolves through the variables too.
 * - **Runtime readers** — this module, for the consumers Tailwind cannot
 *   reach: xterm takes its `fontFamily` and theme colors as JavaScript
 *   options, so the terminal reads the resolved variables off the document
 *   instead of restating the values.
 *
 * Because every consumer resolves through the custom properties, a later
 * user-facing stylesheet can override a token on `:root` and the whole UI
 * (including a freshly created terminal) follows.
 */

/** Read a design token (CSS custom property) resolved on the document root. */
function readToken(name: string): string {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
}

/**
 * The embedded terminal's font stack (`--delta-font-terminal`), defined in
 * `tailwind.config.js` as `fontFamily.terminal`. xterm measures its cell grid
 * from this list, so it deliberately differs from the prose `mono` stack —
 * see the config for the per-OS reasoning.
 */
export function terminalFontFamily(): string {
  return readToken('--delta-font-terminal');
}

/**
 * The embedded terminal's background color (`--delta-color-terminal-bg`),
 * shared with the `bg-terminal-bg` utility so the panel chrome and the xterm
 * canvas can never disagree.
 *
 * The custom property stores a space-separated `R G B` triple (so Tailwind
 * can wrap it as `rgb(var(--…) / <alpha-value>)`), but xterm expects a CSS
 * color string for `theme.background`. Parse the three components and emit
 * a `#RRGGBB` hex literal. If the property is unset, malformed, or the
 * stylesheet has not loaded yet, fall back to slate-900 (`#0f172a`, the
 * canonical value defined in src/index.css) so xterm never sees an empty
 * string.
 */
export function terminalBackground(): string {
  const FALLBACK = '#0f172a';
  const raw = readToken('--delta-color-terminal-bg');
  if (raw === '') {
    return FALLBACK;
  }
  const parts = raw.split(/\s+/);
  if (parts.length < 3) {
    return FALLBACK;
  }
  const components: number[] = [];
  for (let i = 0; i < 3; i++) {
    const n = Number.parseInt(parts[i], 10);
    if (!Number.isFinite(n) || n < 0 || n > 255) {
      return FALLBACK;
    }
    components.push(n);
  }
  return `#${components.map((n) => n.toString(16).padStart(2, '0')).join('')}`;
}
