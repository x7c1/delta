import { beforeEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_SETTINGS_CATEGORY,
  DEFAULT_VISUAL_EFFECTS_SETTING,
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from './settingsStore';
import { DEFAULT_PROVIDER } from '../providers';

describe('settingsStore', () => {
  beforeEach(() => {
    useSettingsStore.setState({
      activeCategory: DEFAULT_SETTINGS_CATEGORY,
      visualEffects: DEFAULT_VISUAL_EFFECTS_SETTING,
      defaultProvider: DEFAULT_PROVIDER,
    });
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

  describe('visualEffects', () => {
    it('defaults to auto on a fresh state', () => {
      expect(useSettingsStore.getState().visualEffects).toBe(
        DEFAULT_VISUAL_EFFECTS_SETTING,
      );
      expect(DEFAULT_VISUAL_EFFECTS_SETTING).toBe('auto');
    });

    it('setVisualEffects updates and persists the setting', () => {
      useSettingsStore.getState().setVisualEffects('off');
      expect(useSettingsStore.getState().visualEffects).toBe('off');

      const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
      expect(raw).not.toBeNull();
      const parsed = JSON.parse(raw ?? '{}');
      expect(parsed.state.visualEffects).toBe('off');
    });

    it('falls back to auto when a foreign value is rehydrated', async () => {
      localStorage.setItem(
        SETTINGS_STORAGE_KEY,
        JSON.stringify({ state: { visualEffects: 'sparkles' }, version: 0 }),
      );
      await useSettingsStore.persist.rehydrate();
      expect(useSettingsStore.getState().visualEffects).toBe(
        DEFAULT_VISUAL_EFFECTS_SETTING,
      );
    });

    it('preserves a valid persisted value across rehydration', async () => {
      localStorage.setItem(
        SETTINGS_STORAGE_KEY,
        JSON.stringify({ state: { visualEffects: 'on' }, version: 0 }),
      );
      await useSettingsStore.persist.rehydrate();
      expect(useSettingsStore.getState().visualEffects).toBe('on');
    });
  });

  it('defaults to Claude as the default provider on a fresh state', () => {
    expect(useSettingsStore.getState().defaultProvider).toBe(DEFAULT_PROVIDER);
    expect(DEFAULT_PROVIDER).toBe('claude');
  });

  it('setDefaultProvider updates and persists the default provider', () => {
    useSettingsStore.getState().setDefaultProvider('codex');
    expect(useSettingsStore.getState().defaultProvider).toBe('codex');
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
    expect(raw).not.toBeNull();
    const parsed = JSON.parse(raw ?? '{}');
    expect(parsed.state.defaultProvider).toBe('codex');
  });

  it('falls back to Claude when a foreign default provider is rehydrated', async () => {
    // A stale/foreign provider token from a different build normalizes to the
    // default so the provider selector never seeds from an unknown value.
    localStorage.setItem(
      SETTINGS_STORAGE_KEY,
      JSON.stringify({ state: { defaultProvider: 'mystery' }, version: 0 }),
    );
    await useSettingsStore.persist.rehydrate();
    expect(useSettingsStore.getState().defaultProvider).toBe(DEFAULT_PROVIDER);
  });

  it('preserves a valid persisted default provider across rehydration', async () => {
    localStorage.setItem(
      SETTINGS_STORAGE_KEY,
      JSON.stringify({ state: { defaultProvider: 'codex' }, version: 0 }),
    );
    await useSettingsStore.persist.rehydrate();
    expect(useSettingsStore.getState().defaultProvider).toBe('codex');
  });
});
