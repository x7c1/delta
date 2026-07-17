import { useEffect, type ReactNode } from 'react';
import { useSettingsStore } from '../store/settingsStore';
import {
  resolveVisualEffects,
  writeDocumentEffects,
  type ResolvedVisualEffects,
} from './visualEffects';

/** Resolve the setting against the live browser environment. */
function resolveFromEnv(
  setting: Parameters<typeof resolveVisualEffects>[0],
): ResolvedVisualEffects {
  const userAgent = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  const platform = typeof navigator !== 'undefined' ? navigator.platform : '';
  return resolveVisualEffects(setting, userAgent, platform);
}

export interface VisualEffectsProviderProps {
  children: ReactNode;
}

/**
 * Application-wide provider that mirrors how the theme reaches CSS. The theme
 * provider stamps `<html data-theme="…">`; this one derives the effective
 * `rich`/`flat` look from the persisted {@link useSettingsStore} setting plus
 * the runtime environment (`navigator`) and stamps `<html data-effects="…">`
 * the same way, updating live when the setting changes (no reload required).
 *
 * Mount it once at the application root so the stamping effect runs in exactly
 * one place. The decorative CSS reads only the stamped attribute, so the
 * resolved value is not exposed to descendants — the store remains the single
 * source of truth for anything that needs the setting in JS.
 */
export function VisualEffectsProvider({ children }: VisualEffectsProviderProps) {
  const setting = useSettingsStore((state) => state.visualEffects);
  const resolved = resolveFromEnv(setting);

  useEffect(() => {
    writeDocumentEffects(resolved);
  }, [resolved]);

  return <>{children}</>;
}
