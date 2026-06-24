import { beforeEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_SETTINGS_CATEGORY,
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from './settingsStore';

describe('settingsStore', () => {
  beforeEach(() => {
    useSettingsStore.setState({ activeCategory: DEFAULT_SETTINGS_CATEGORY });
    localStorage.removeItem(SETTINGS_STORAGE_KEY);
  });

  it('defaults to launch-options on a fresh state', () => {
    expect(useSettingsStore.getState().activeCategory).toBe(
      DEFAULT_SETTINGS_CATEGORY,
    );
    expect(DEFAULT_SETTINGS_CATEGORY).toBe('launch-options');
  });

  it('setActiveCategory updates the active category', () => {
    useSettingsStore.getState().setActiveCategory('scan-roots');
    expect(useSettingsStore.getState().activeCategory).toBe('scan-roots');

    useSettingsStore.getState().setActiveCategory('launch-options');
    expect(useSettingsStore.getState().activeCategory).toBe('launch-options');
  });

  it('persists the active category to localStorage', () => {
    useSettingsStore.getState().setActiveCategory('scan-roots');
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
    expect(raw).not.toBeNull();
    const parsed = JSON.parse(raw ?? '{}');
    expect(parsed.state.activeCategory).toBe('scan-roots');
  });

  it('falls back to the default when a foreign value is rehydrated', async () => {
    // A foreign value from a different build or a typo: the rehydration hook
    // normalizes it to the default so the right pane never lands on an
    // unknown category.
    localStorage.setItem(
      SETTINGS_STORAGE_KEY,
      JSON.stringify({ state: { activeCategory: 'mystery' }, version: 0 }),
    );
    await useSettingsStore.persist.rehydrate();
    expect(useSettingsStore.getState().activeCategory).toBe(
      DEFAULT_SETTINGS_CATEGORY,
    );
  });

  it('preserves a valid persisted value across rehydration', async () => {
    localStorage.setItem(
      SETTINGS_STORAGE_KEY,
      JSON.stringify({ state: { activeCategory: 'scan-roots' }, version: 0 }),
    );
    await useSettingsStore.persist.rehydrate();
    expect(useSettingsStore.getState().activeCategory).toBe('scan-roots');
  });
});
