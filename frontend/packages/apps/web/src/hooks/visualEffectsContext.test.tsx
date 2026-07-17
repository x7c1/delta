import { act, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_VISUAL_EFFECTS_SETTING,
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../store/settingsStore';
import { VisualEffectsProvider } from './visualEffectsContext';

describe('VisualEffectsProvider', () => {
  beforeEach(() => {
    useSettingsStore.setState({ visualEffects: DEFAULT_VISUAL_EFFECTS_SETTING });
    localStorage.removeItem(SETTINGS_STORAGE_KEY);
    delete document.documentElement.dataset.effects;
  });

  afterEach(() => {
    useSettingsStore.setState({ visualEffects: DEFAULT_VISUAL_EFFECTS_SETTING });
    delete document.documentElement.dataset.effects;
  });

  it('stamps data-effects on the document root reflecting the effective value', () => {
    // The explicit settings resolve independently of the (jsdom) UA, so the
    // stamped value is deterministic regardless of the test environment.
    useSettingsStore.setState({ visualEffects: 'off' });
    render(
      <VisualEffectsProvider>
        <span />
      </VisualEffectsProvider>,
    );
    expect(document.documentElement.dataset.effects).toBe('flat');
  });

  it('updates the attribute live when the setting changes (no reload)', () => {
    useSettingsStore.setState({ visualEffects: 'off' });
    render(
      <VisualEffectsProvider>
        <span />
      </VisualEffectsProvider>,
    );
    expect(document.documentElement.dataset.effects).toBe('flat');

    // Flip the store; the provider re-renders, re-resolves, and re-stamps
    // without any remount / reload.
    act(() => {
      useSettingsStore.getState().setVisualEffects('on');
    });
    expect(document.documentElement.dataset.effects).toBe('rich');
  });
});
