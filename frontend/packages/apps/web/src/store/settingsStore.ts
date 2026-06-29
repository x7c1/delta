import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

/**
 * The Settings dialog category ids — each one corresponds to a left-rail entry
 * and a right-pane content view in the VS Code-style 2-pane layout. Adding a
 * new top-level category is a single entry in both this union and the
 * registry that drives the rail (see {@link SettingsView}).
 */
export type SettingsCategoryId = 'launch-options' | 'scan-roots' | 'appearance';

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
];

export interface SettingsState {
  /**
   * Which Settings category the dialog shows in its right pane. Persisted to
   * localStorage so a reload (or a dialog close/reopen) restores the last
   * choice. Restored on rehydration; an unknown value falls back to the
   * default (see {@link DEFAULT_SETTINGS_CATEGORY}).
   */
  activeCategory: SettingsCategoryId;
  setActiveCategory: (category: SettingsCategoryId) => void;
}

/** localStorage key for the persisted settings dialog state slice. */
export const SETTINGS_STORAGE_KEY = 'delta-settings';

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      activeCategory: DEFAULT_SETTINGS_CATEGORY,
      setActiveCategory: (activeCategory) => set({ activeCategory }),
    }),
    {
      name: SETTINGS_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      onRehydrateStorage: () => (state) => {
        if (state && !SETTINGS_CATEGORY_IDS.includes(state.activeCategory)) {
          state.activeCategory = DEFAULT_SETTINGS_CATEGORY;
        }
      },
    },
  ),
);
