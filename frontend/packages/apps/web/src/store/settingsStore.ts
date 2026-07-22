import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import type { AgentProvider } from '@delta/wire-gen';

/**
 * The Settings dialog category ids — each one corresponds to a left-rail entry
 * and a right-pane content view in the VS Code-style 2-pane layout. Adding a
 * new top-level category is a single entry in both this union and the
 * registry that drives the rail (see {@link SettingsView}).
 */
export type SettingsCategoryId =
  | 'launch-options'
  | 'scan-roots'
  | 'appearance'
  | 'default-provider';

/**
 * The default category on a fresh install: Launch options. It is the older,
 * more prominent of the two existing categories, so leading with it preserves
 * the dialog's prior landing experience for users opening Settings for the
 * first time after this restructure.
 */
export const DEFAULT_SETTINGS_CATEGORY: SettingsCategoryId = 'launch-options';

/**
 * The valid {@link SettingsCategoryId} values, used by the persistence
 * hydration step to fall back to the default when a foreign value lands in
 * localStorage (a different build, a typo, an experiment that left a trail).
 */
const SETTINGS_CATEGORY_IDS: readonly SettingsCategoryId[] = [
  'launch-options',
  'scan-roots',
  'appearance',
  'default-provider',
];

/**
 * The user's decorative-rendering preference. Some rich rendering (card
 * drop-shadows, the timeline landing wash) is cheap on Chromium/Safari but
 * costs WebKitGTK (Linux) a full raster/paint on every repaint, which reads as
 * input lag. This three-way setting lets the user keep the rich look, force
 * the flat look, or defer to a platform-aware default (see the resolver in
 * `hooks/visualEffects.ts`).
 *
 * - `auto` — resolve from the environment: flat on Linux WebKit, rich anywhere
 *   else.
 * - `on`   — always rich, regardless of platform.
 * - `off`  — always flat, regardless of platform.
 */
export type VisualEffectsSetting = 'auto' | 'on' | 'off';

/**
 * The default on a fresh install: defer to the platform. Every non-Linux-WebKit
 * environment resolves this to the rich look, so today's users see no change;
 * only Linux WebKit (Epiphany / a WebKitGTK shell) flips to flat automatically.
 */
export const DEFAULT_VISUAL_EFFECTS_SETTING: VisualEffectsSetting = 'auto';

/**
 * The valid {@link VisualEffectsSetting} values, used by the persistence
 * hydration step to fall back to the default when a foreign value lands in
 * localStorage (a different build, a typo, an experiment that left a trail).
 */
const VISUAL_EFFECTS_SETTINGS: readonly VisualEffectsSetting[] = [
  'auto',
  'on',
  'off',
];

/**
 * The valid {@link AgentProvider} values, used by the persistence hydration
 * step to fall back to {@link DEFAULT_PROVIDER} when a foreign value lands in
 * localStorage (a stale build, a typo, a value from another workspace). Kept in
 * sync with the wire `AgentProvider` union by construction: the `satisfies`
 * clause makes TypeScript reject the tuple if it drifts from the union.
 */
const AGENT_PROVIDERS = ['claude', 'codex'] as const satisfies readonly AgentProvider[];

/**
 * The provider a new session defaults to before the user picks one for a given
 * session. Claude on a fresh install; the new-session provider selector seeds
 * its initial value from the persisted {@link SettingsState.defaultProvider},
 * which starts here.
 */
export const DEFAULT_PROVIDER: AgentProvider = 'claude';

export interface SettingsState {
  /**
   * Which Settings category the dialog shows in its right pane. Persisted to
   * localStorage so a reload (or a dialog close/reopen) restores the last
   * choice. Restored on rehydration; an unknown value falls back to the
   * default (see {@link DEFAULT_SETTINGS_CATEGORY}).
   */
  activeCategory: SettingsCategoryId;
  setActiveCategory: (category: SettingsCategoryId) => void;
  /**
   * Decorative-rendering preference. Persisted to localStorage; restored on
   * rehydration with an unknown value falling back to the default (see
   * {@link DEFAULT_VISUAL_EFFECTS_SETTING}). The effective `rich`/`flat` value
   * is derived from this plus the environment by the resolver in
   * `hooks/visualEffects.ts` and stamped onto `<html data-effects="…">`.
   */
  visualEffects: VisualEffectsSetting;
  setVisualEffects: (visualEffects: VisualEffectsSetting) => void;
  /**
   * The AI-agent provider a new session starts on by default. It seeds the
   * new-session provider selector's initial value (still per-session
   * overridable via that selector). Persisted to localStorage so the choice
   * survives reloads; an unknown value falls back to {@link DEFAULT_PROVIDER}
   * on rehydration.
   */
  defaultProvider: AgentProvider;
  setDefaultProvider: (provider: AgentProvider) => void;
}

/** localStorage key for the persisted settings dialog state slice. */
export const SETTINGS_STORAGE_KEY = 'delta-settings';

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      activeCategory: DEFAULT_SETTINGS_CATEGORY,
      setActiveCategory: (activeCategory) => set({ activeCategory }),
      visualEffects: DEFAULT_VISUAL_EFFECTS_SETTING,
      setVisualEffects: (visualEffects) => set({ visualEffects }),
      defaultProvider: DEFAULT_PROVIDER,
      setDefaultProvider: (defaultProvider) => set({ defaultProvider }),
    }),
    {
      name: SETTINGS_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      onRehydrateStorage: () => (state) => {
        if (!state) {
          return;
        }
        if (!SETTINGS_CATEGORY_IDS.includes(state.activeCategory)) {
          state.activeCategory = DEFAULT_SETTINGS_CATEGORY;
        }
        if (!VISUAL_EFFECTS_SETTINGS.includes(state.visualEffects)) {
          state.visualEffects = DEFAULT_VISUAL_EFFECTS_SETTING;
        }
        if (!AGENT_PROVIDERS.includes(state.defaultProvider)) {
          state.defaultProvider = DEFAULT_PROVIDER;
        }
      },
    },
  ),
);
