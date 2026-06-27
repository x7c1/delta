import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_THEME_ID } from '../themes/registry';
import {
  SYSTEM_PREFERENCE,
  THEME_PREFERENCE_STORAGE_KEY,
  useTheme,
} from './useTheme';

/**
 * A controllable `matchMedia` stub: tests flip `matches` and then dispatch a
 * `change` event to mimic a real OS toggle. Mirrors the matchMedia stubbing
 * pattern other tests in this app use (see WorkspaceScreen.test.tsx), but
 * extended with a working listener registry so `'system'` reactivity is
 * exercisable.
 */
function installMatchMediaStub(initialDarkMatches: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const state: { matches: boolean } = { matches: initialDarkMatches };
  const mql: MediaQueryList = {
    matches: state.matches,
    media: '(prefers-color-scheme: dark)',
    onchange: null,
    addEventListener: ((type: string, listener: EventListener) => {
      if (type === 'change') {
        listeners.add(listener as (event: MediaQueryListEvent) => void);
      }
    }) as MediaQueryList['addEventListener'],
    removeEventListener: ((type: string, listener: EventListener) => {
      if (type === 'change') {
        listeners.delete(listener as (event: MediaQueryListEvent) => void);
      }
    }) as MediaQueryList['removeEventListener'],
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  };
  // matchMedia returns the same object every call so the listener registry is
  // shared between the initial read and the effect's subscribe.
  vi.stubGlobal('matchMedia', () => {
    // Refresh `matches` from `state` each call so flips propagate.
    Object.defineProperty(mql, 'matches', { get: () => state.matches });
    return mql;
  });

  return {
    setDark(next: boolean) {
      state.matches = next;
      const event = { matches: next } as MediaQueryListEvent;
      for (const listener of listeners) {
        listener(event);
      }
    },
  };
}

describe('useTheme', () => {
  beforeEach(() => {
    localStorage.removeItem(THEME_PREFERENCE_STORAGE_KEY);
    delete document.documentElement.dataset.theme;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('resolves an explicit "dark" preference and writes data-theme="dark"', () => {
    installMatchMediaStub(false);
    localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, 'dark');

    const { result } = renderHook(() => useTheme());

    expect(result.current.preference).toBe('dark');
    expect(result.current.resolved).toBe('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('resolves an explicit "light" preference and writes data-theme="light"', () => {
    installMatchMediaStub(true);
    localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, 'light');

    const { result } = renderHook(() => useTheme());

    expect(result.current.preference).toBe('light');
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.dataset.theme).toBe('light');
  });

  it('resolves "system" via matchMedia: prefers-dark → dark', () => {
    installMatchMediaStub(true);
    localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, SYSTEM_PREFERENCE);

    const { result } = renderHook(() => useTheme());

    expect(result.current.preference).toBe(SYSTEM_PREFERENCE);
    expect(result.current.resolved).toBe('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('resolves "system" via matchMedia: prefers-light → light', () => {
    installMatchMediaStub(false);
    localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, SYSTEM_PREFERENCE);

    const { result } = renderHook(() => useTheme());

    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.dataset.theme).toBe('light');
  });

  it('re-resolves when matchMedia fires a change event under "system"', () => {
    const media = installMatchMediaStub(false);
    localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, SYSTEM_PREFERENCE);

    const { result } = renderHook(() => useTheme());
    expect(result.current.resolved).toBe('light');

    act(() => {
      media.setDark(true);
    });
    expect(result.current.resolved).toBe('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');

    act(() => {
      media.setDark(false);
    });
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.dataset.theme).toBe('light');
  });

  it('falls back to system when the stored value is unknown', () => {
    installMatchMediaStub(false);
    localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, 'mystery-theme');

    const { result } = renderHook(() => useTheme());

    // Unknown stored values are treated as "no preference" → SYSTEM.
    expect(result.current.preference).toBe(SYSTEM_PREFERENCE);
    // With prefers-dark = false, system resolves to 'light'; the test's intent
    // is to prove that a foreign string never lands on `<html>` directly.
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.dataset.theme).toBe('light');
  });

  it('falls back to DEFAULT_THEME_ID when matchMedia is unavailable under "system"', () => {
    // No matchMedia at all → readSystemTheme returns DEFAULT_THEME_ID.
    vi.stubGlobal('matchMedia', undefined);
    // No stored preference → readStoredPreference returns SYSTEM.
    const { result } = renderHook(() => useTheme());

    expect(result.current.preference).toBe(SYSTEM_PREFERENCE);
    expect(result.current.resolved).toBe(DEFAULT_THEME_ID);
    expect(document.documentElement.dataset.theme).toBe(DEFAULT_THEME_ID);
  });

  it('setPreference("light") persists to localStorage and updates resolved', () => {
    installMatchMediaStub(true);
    // Start on the SYSTEM default so the explicit pick is observable.
    const { result } = renderHook(() => useTheme());
    expect(result.current.preference).toBe(SYSTEM_PREFERENCE);

    act(() => {
      result.current.setPreference('light');
    });

    expect(result.current.preference).toBe('light');
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(localStorage.getItem(THEME_PREFERENCE_STORAGE_KEY)).toBe('light');
  });

  it('exposes DEFAULT_THEME_ID as a real registered id', () => {
    // Guard against the registry losing the default id during a future edit.
    expect(['dark', 'light']).toContain(DEFAULT_THEME_ID);
  });
});
