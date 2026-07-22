import type { VisualEffectsSetting } from '../store/settingsStore';

/**
 * Resolution of the {@link VisualEffectsSetting} against the environment.
 *
 * The setting is a user intent (`auto | on | off`); the effective value is the
 * concrete look the CSS gates on, stamped onto `<html data-effects="…">`:
 *
 * - `rich` — decorative rendering on (card drop-shadows, the timeline landing
 *   wash). Today's look everywhere except Linux WebKit under `auto`.
 * - `flat` — those two decorative costs suppressed. WebKitGTK (Linux) pays a
 *   full raster/paint for both on every repaint, which reads as input lag;
 *   Chromium and macOS WebKit do not, so the flat look is only the `auto`
 *   default there.
 *
 * The resolver is a pure function of `(setting, userAgent, platform)` so it is
 * unit-testable without DOM globals; the DOM reads (`navigator`, `document`)
 * live in {@link VisualEffectsProvider}.
 */
export type ResolvedVisualEffects = 'rich' | 'flat';

/**
 * Whether the environment is a WebKit-engined browser running on Linux — the
 * one combination the `auto` setting resolves to `flat`.
 *
 * "WebKit-engined" means the UA carries the `AppleWebKit` token but none of the
 * Chromium-family markers (`Chrome/`, `Chromium/`, `Edg/`) — every
 * Chromium-family browser also carries `AppleWebKit`, so the engine can only be
 * distinguished by the *absence* of those markers. This matches macOS
 * Safari/WKWebView and a Linux WebKitGTK shell (Epiphany) but not Chrome/Edge
 * (Chromium markers present) or Firefox (`Gecko`, no `AppleWebKit`).
 *
 * The Linux check reads `platform` (e.g. `navigator.platform` → `"Linux
 * x86_64"`) rather than the UA so it stays independent of the engine token.
 */
export function isLinuxWebKit(userAgent: string, platform: string): boolean {
  const webKitEngined =
    /AppleWebKit/.test(userAgent) && !/(Chrome|Chromium|Edg)\//.test(userAgent);
  const linux = /Linux/i.test(platform);
  return webKitEngined && linux;
}

/**
 * Map the user's setting plus the environment to the effective look. `on`/`off`
 * are absolute (they win over the platform); `auto` defers to
 * {@link isLinuxWebKit}.
 */
export function resolveVisualEffects(
  setting: VisualEffectsSetting,
  userAgent: string,
  platform: string,
): ResolvedVisualEffects {
  if (setting === 'on') {
    return 'rich';
  }
  if (setting === 'off') {
    return 'flat';
  }
  return isLinuxWebKit(userAgent, platform) ? 'flat' : 'rich';
}

/**
 * Write the effective look onto `<html data-effects="…">` so the CSS gates in
 * `src/index.css` take over. No-op when there is no document (SSR-style
 * environments). Idempotent, so it is safe to call from an effect on every
 * resolution change.
 */
export function writeDocumentEffects(resolved: ResolvedVisualEffects): void {
  if (typeof document === 'undefined') {
    return;
  }
  document.documentElement.dataset.effects = resolved;
}
