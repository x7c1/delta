/**
 * Theme registry — the single source of truth for the list of selectable
 * themes.
 *
 * How to add a new theme (it is a 2-file change, no picker-code edits):
 *   1. Add a `:root[data-theme="<id>"]` block in `src/index.css` defining
 *      every semantic color CSS variable the theme contract specifies.
 *      Theme-fixed tokens (`terminal-*`, `highlight-wash`) must keep their
 *      existing values; see the contract at the top of `src/index.css`.
 *   2. Add a {@link ThemeMeta} entry to {@link THEMES} below.
 *   3. The Appearance picker (see `AppearanceSection` in
 *      `src/features/settings/SettingsView.tsx`) enumerates `THEMES`, so the
 *      new option appears in the UI automatically.
 *
 * The `sepia` entry below is a working example of the recipe, showing both
 * how to register a new theme and what a non-built-in entry looks like.
 *
 * {@link ThemeId} is intentionally an open string shape (`string & {}`) so
 * call sites can accept future theme ids without forcing this file's union to
 * grow first. The narrow `'dark' | 'light'` arm still gives editors
 * autocomplete for the two built-in ids.
 */

export type ThemeId = 'dark' | 'light' | (string & {});

/** Metadata describing a single selectable theme. */
export interface ThemeMeta {
  /** Stable id; matches `:root[data-theme="<id>"]` in src/index.css. */
  id: ThemeId;
  /** Human-readable label for theme pickers. */
  displayName: string;
  /**
   * True when the theme's surface is dark — consumers such as embedded
   * widgets (xterm, syntax highlighters) that need to pick a "kind" of
   * palette without inspecting individual tokens key off this flag.
   */
  isDark: boolean;
}

/**
 * The ordered list of selectable themes. Order here defines the order a
 * theme-picker UI presents them in. Mirrors the `:root[data-theme="..."]`
 * blocks in src/index.css — keep in sync when adding/removing a theme.
 */
export const THEMES: ReadonlyArray<ThemeMeta> = [
  { id: 'dark', displayName: 'Dark', isDark: true },
  { id: 'light', displayName: 'Light', isDark: false },
  { id: 'sepia', displayName: 'Sepia', isDark: false },
];

/**
 * The theme used when no preference is stored and the OS does not signal a
 * preference (and as the safe fallback for an unrecognized stored value).
 * Light, because the existing UI chrome is light (white / slate-50 /
 * slate-100 surfaces with slate-700/800/900 text); only the embedded
 * terminal is dark.
 */
export const DEFAULT_THEME_ID: ThemeId = 'light';

/** Look up a registered theme by id, or `undefined` if not registered. */
export function findTheme(id: string): ThemeMeta | undefined {
  return THEMES.find((t) => t.id === id);
}
