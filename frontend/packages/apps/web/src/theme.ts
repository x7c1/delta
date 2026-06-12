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
 * The embedded terminal's background color (`--delta-terminal-bg`), shared
 * with the `bg-terminal-bg` utility so the panel chrome and the xterm canvas
 * can never disagree.
 */
export function terminalBackground(): string {
  return readToken('--delta-terminal-bg');
}
