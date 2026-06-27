import { useCallback, useEffect, useState } from 'react';
import {
  DEFAULT_THEME_ID,
  findTheme,
  type ThemeId,
} from '../themes/registry';

/**
 * React hook that drives the app's active theme.
 *
 * The hook owns three pieces of state:
 *
 * - `preference` — what the user picked: a concrete theme id or `'system'`
 *   ("follow the OS"). Persisted to `localStorage` so it survives a reload.
 * - `resolved`   — the concrete theme id currently applied, never `'system'`.
 *   When `preference` is `'system'`, `resolved` mirrors
 *   `prefers-color-scheme` and re-renders when that media query flips.
 * - `setPreference(p)` — update the user's pick and persist it.
 *
 * The hook also writes `document.documentElement.dataset.theme = resolved` in
 * an effect so the active `:root[data-theme="..."]` block in `src/index.css`
 * takes over. The same write is performed by an inline `<script>` in
 * `index.html` before the React bundle loads, which avoids a flash of the
 * default theme on reload — see that file for the mirrored logic.
 *
 * `localStorage` and `matchMedia` access is wrapped in try/catch because
 * Safari private mode throws on storage access and SSR-style environments
 * may lack `window` entirely. A failure is treated as "no stored
 * preference" / "no system signal".
 */

/** localStorage key for the persisted theme preference. */
export const THEME_PREFERENCE_STORAGE_KEY = 'delta.preferences.theme';

/** Stored "follow the OS preference" sentinel. */
export const SYSTEM_PREFERENCE = 'system' as const;

export type ThemePreference = ThemeId | typeof SYSTEM_PREFERENCE;

/** Match-media query that signals OS-level dark mode. */
const PREFERS_DARK_QUERY = '(prefers-color-scheme: dark)';

/**
 * Read the stored preference, returning the {@link SYSTEM_PREFERENCE}
 * sentinel as the default. Unknown values (left over from a previous build,
 * or hand-edited) fall back to system so a new install behaves like a fresh
 * one. Returns `SYSTEM_PREFERENCE` on any storage failure.
 */
function readStoredPreference(): ThemePreference {
  if (typeof window === 'undefined') {
    return SYSTEM_PREFERENCE;
  }
  try {
    const raw = window.localStorage.getItem(THEME_PREFERENCE_STORAGE_KEY);
    if (raw === null) {
      return SYSTEM_PREFERENCE;
    }
    if (raw === SYSTEM_PREFERENCE) {
      return SYSTEM_PREFERENCE;
    }
    return findTheme(raw) !== undefined ? (raw as ThemeId) : SYSTEM_PREFERENCE;
  } catch {
    return SYSTEM_PREFERENCE;
  }
}

/** Resolve `prefers-color-scheme: dark` → concrete id, with a safe default. */
function readSystemTheme(): ThemeId {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return DEFAULT_THEME_ID;
  }
  try {
    return window.matchMedia(PREFERS_DARK_QUERY).matches ? 'dark' : 'light';
  } catch {
    return DEFAULT_THEME_ID;
  }
}

/** Resolve a preference to a concrete id (never `'system'`). */
function resolvePreference(preference: ThemePreference): ThemeId {
  return preference === SYSTEM_PREFERENCE ? readSystemTheme() : preference;
}

export interface UseThemeResult {
  preference: ThemePreference;
  resolved: ThemeId;
  setPreference: (preference: ThemePreference) => void;
}

export function useTheme(): UseThemeResult {
  const [preference, setPreferenceState] = useState<ThemePreference>(
    readStoredPreference,
  );
  const [resolved, setResolved] = useState<ThemeId>(() =>
    resolvePreference(readStoredPreference()),
  );

  // When the user picks an explicit theme, `resolved` follows directly. When
  // the user picks `'system'`, also subscribe to the media query so a later
  // OS toggle re-renders without requiring a reload.
  useEffect(() => {
    if (preference !== SYSTEM_PREFERENCE) {
      setResolved(preference);
      return;
    }
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
      setResolved(DEFAULT_THEME_ID);
      return;
    }
    const mql = window.matchMedia(PREFERS_DARK_QUERY);
    const sync = () => setResolved(mql.matches ? 'dark' : 'light');
    sync();
    mql.addEventListener('change', sync);
    return () => mql.removeEventListener('change', sync);
  }, [preference]);

  // Push the resolved id onto `<html data-theme="...">` so the matching
  // `:root[data-theme="..."]` block in src/index.css takes effect.
  useEffect(() => {
    if (typeof document === 'undefined') {
      return;
    }
    document.documentElement.dataset.theme = resolved;
  }, [resolved]);

  const setPreference = useCallback((next: ThemePreference) => {
    setPreferenceState(next);
    if (typeof window === 'undefined') {
      return;
    }
    try {
      window.localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, next);
    } catch {
      // Storage may be unavailable (Safari private mode, quota). The in-memory
      // preference is still honored for the rest of the page session.
    }
  }, []);

  return { preference, resolved, setPreference };
}
