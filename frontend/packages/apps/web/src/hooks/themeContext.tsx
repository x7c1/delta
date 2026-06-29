import { createContext, useContext, type ReactNode } from 'react';
import { useTheme, type UseThemeResult } from './useTheme';

/**
 * Application-wide singleton wrapper around {@link useTheme}.
 *
 * `useTheme` owns the active theme's state, the `prefers-color-scheme`
 * subscription, the `localStorage` write, and the `<html data-theme="...">`
 * update — running its effects more than once would mean redundant
 * subscriptions and split state between consumers. Mount {@link ThemeProvider}
 * once at the application root so the hook is exercised in exactly one place,
 * then read the shared `{preference, resolved, setPreference}` from any
 * descendant via {@link useThemeContext}.
 *
 * `useTheme` itself remains usable directly (e.g. for unit tests that render
 * the hook in isolation). In the running app, consumers should always go
 * through this context so the picker and the embedded terminal observe the
 * same state and never disagree about which theme is active.
 */

const ThemeContext = createContext<UseThemeResult | null>(null);

export interface ThemeProviderProps {
  children: ReactNode;
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  const value = useTheme();
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

/**
 * Read the application-wide theme state. Throws if called outside a
 * {@link ThemeProvider} so a missing wrap is caught at mount time rather than
 * silently returning stale state from a parallel `useTheme` call.
 */
export function useThemeContext(): UseThemeResult {
  const value = useContext(ThemeContext);
  if (value === null) {
    throw new Error('useThemeContext must be used within a <ThemeProvider>');
  }
  return value;
}
