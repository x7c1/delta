import { cn } from '@delta/ui-kit';
import { useThemeContext } from '../../hooks/themeContext';
import {
  type VisualEffectsSetting,
  useSettingsStore,
} from '../../store/settingsStore';
import { SYSTEM_PREFERENCE, type ThemePreference } from '../../hooks/useTheme';
import { THEMES } from '../../themes/registry';

/**
 * Appearance category content: pick which theme drives the UI. Options are
 * sourced from the theme registry (every registered `:root[data-theme="…"]`
 * block in src/index.css) plus a `System` option that follows
 * `prefers-color-scheme`. Selection writes through {@link useThemeContext}'s
 * setter, which persists to localStorage and updates `<html data-theme="…">`
 * — the surrounding UI (and the embedded xterm canvas) re-resolves its
 * design tokens in the same tick.
 *
 * The control is a radio group so each option is independently focusable for
 * keyboard navigation and screen readers announce the role correctly. The
 * highlight reflects the user's stated preference (including `system`)
 * rather than the resolved id, so picking `System` stays visibly checked
 * regardless of which concrete theme the OS currently signals.
 */
export function AppearanceSection() {
  const { preference, setPreference } = useThemeContext();
  const visualEffects = useSettingsStore((state) => state.visualEffects);
  const setVisualEffects = useSettingsStore((state) => state.setVisualEffects);

  const options: { value: ThemePreference; label: string; hint: string }[] = [
    ...THEMES.map((theme) => ({
      value: theme.id as ThemePreference,
      label: theme.displayName,
      hint: theme.isDark ? 'Dark surfaces' : 'Light surfaces',
    })),
    {
      value: SYSTEM_PREFERENCE,
      label: 'System',
      hint: 'Follow the OS preference',
    },
  ];

  // Visual-effects options. "Auto" defers to the platform (flat on Linux
  // WebKit, rich elsewhere); "On"/"Off" force the look regardless of platform.
  const effectsOptions: {
    value: VisualEffectsSetting;
    label: string;
    hint: string;
  }[] = [
    {
      value: 'auto',
      label: 'Auto (platform default)',
      hint: 'Flat on Linux WebKit, rich elsewhere',
    },
    { value: 'on', label: 'On', hint: 'Always show the rich look' },
    { value: 'off', label: 'Off', hint: 'Always use the flat look' },
  ];

  return (
    <section className="w-full" data-testid="appearance-section">
      <h3 className="mb-1 text-secondary font-semibold text-fg">Appearance</h3>
      <p className="mb-4 text-caption text-fg-muted">
        Choose the theme used across the app. <span className="font-medium">System</span>{' '}
        follows your operating system&apos;s color-scheme preference and updates
        live when it changes.
      </p>
      <div
        role="radiogroup"
        aria-labelledby="appearance-section-heading"
        className="flex flex-col gap-2 rounded-lg border border-border-default bg-surface-elevated p-3"
        data-testid="appearance-theme-options"
      >
        <span id="appearance-section-heading" className="sr-only">
          Theme preference
        </span>
        {options.map((option) => {
          const selected = preference === option.value;
          return (
            <label
              key={option.value}
              className={cn(
                'flex cursor-pointer items-center gap-3 rounded border px-3 py-2 text-secondary transition',
                selected
                  ? 'border-accent bg-accent/10 text-fg ring-1 ring-accent/30'
                  : 'border-border-default text-fg hover:bg-surface',
              )}
              data-testid={`appearance-option-${option.value}`}
            >
              <input
                type="radio"
                name="appearance-theme"
                value={option.value}
                checked={selected}
                onChange={() => setPreference(option.value)}
                className="h-3.5 w-3.5 accent-accent"
              />
              <span className="flex flex-1 flex-col">
                <span className="font-medium">{option.label}</span>
                <span className="text-caption text-fg-muted">{option.hint}</span>
              </span>
            </label>
          );
        })}
      </div>

      <h4 className="mb-1 mt-6 text-secondary font-semibold text-fg">
        Visual effects
      </h4>
      <p className="mb-4 text-caption text-fg-muted">
        Card shadows and the timeline landing flash. These are cheap on most
        browsers but cost some (notably Linux WebKit) a repaint that reads as
        lag; <span className="font-medium">Auto</span> keeps the rich look
        everywhere except where it hurts.
      </p>
      <div
        role="radiogroup"
        aria-labelledby="appearance-effects-heading"
        className="flex flex-col gap-2 rounded-lg border border-border-default bg-surface-elevated p-3"
        data-testid="appearance-effects-options"
      >
        <span id="appearance-effects-heading" className="sr-only">
          Visual effects
        </span>
        {effectsOptions.map((option) => {
          const selected = visualEffects === option.value;
          return (
            <label
              key={option.value}
              className={cn(
                'flex cursor-pointer items-center gap-3 rounded border px-3 py-2 text-secondary transition',
                selected
                  ? 'border-accent bg-accent/10 text-fg ring-1 ring-accent/30'
                  : 'border-border-default text-fg hover:bg-surface',
              )}
              data-testid={`appearance-effects-option-${option.value}`}
            >
              <input
                type="radio"
                name="appearance-effects"
                value={option.value}
                checked={selected}
                onChange={() => setVisualEffects(option.value)}
                className="h-3.5 w-3.5 accent-accent"
              />
              <span className="flex flex-1 flex-col">
                <span className="font-medium">{option.label}</span>
                <span className="text-caption text-fg-muted">{option.hint}</span>
              </span>
            </label>
          );
        })}
      </div>
    </section>
  );
}
