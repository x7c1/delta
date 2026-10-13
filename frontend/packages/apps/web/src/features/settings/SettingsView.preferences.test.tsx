/**
 * The default provider and appearance categories.
 *
 * Shared fixtures live in SettingsView.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { createHandlers } from '@delta/api-mocks';
import {
  SYSTEM_PREFERENCE,
  THEME_PREFERENCE_STORAGE_KEY,
} from '../../hooks/useTheme';
import {
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../../store/settingsStore';
import {
  installMatchMediaStub,
  renderSettings,
  resetSettingsState,
  server,
  unstubGlobals,
} from './SettingsView.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

describe('SettingsView', () => {
  beforeEach(resetSettingsState);
  afterEach(unstubGlobals);

  describe('Default provider section', () => {
    function switchToDefaultProvider() {
      fireEvent.click(screen.getByTestId('settings-category-default-provider'));
    }

    it('lists both providers with their hue-tinted names', () => {
      renderSettings();
      switchToDefaultProvider();
      const group = screen.getByTestId('default-provider-options');
      const radios = within(group).getAllByRole('radio');
      expect(radios.map((r) => (r as HTMLInputElement).value)).toEqual([
        'claude',
        'codex',
      ]);
      // Each option writes its product name through the shared ProviderName,
      // which tints the words in the provider hue.
      const claude = screen.getByTestId('default-provider-option-claude');
      const codex = screen.getByTestId('default-provider-option-codex');
      expect(
        within(claude).getByText('Claude Code').className,
      ).toContain('text-provider-claude');
      expect(within(codex).getByText('Codex').className).toContain(
        'text-provider-codex',
      );
    });

    it('highlights the current default (Claude on a fresh install)', () => {
      renderSettings();
      switchToDefaultProvider();
      const claudeRadio = within(
        screen.getByTestId('default-provider-option-claude'),
      ).getByRole('radio');
      const codexRadio = within(
        screen.getByTestId('default-provider-option-codex'),
      ).getByRole('radio');
      expect(claudeRadio).toBeChecked();
      expect(codexRadio).not.toBeChecked();
    });

    it('changes the default provider and persists it to localStorage', () => {
      renderSettings();
      switchToDefaultProvider();
      const codexRadio = within(
        screen.getByTestId('default-provider-option-codex'),
      ).getByRole('radio');
      fireEvent.click(codexRadio);

      expect(codexRadio).toBeChecked();
      expect(useSettingsStore.getState().defaultProvider).toBe('codex');
      // The persist middleware wraps state under `{ state, version }`.
      const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
      expect(raw).not.toBeNull();
      const parsed = JSON.parse(raw ?? '{}');
      expect(parsed.state.defaultProvider).toBe('codex');
    });
  });

  describe('Appearance section', () => {
    function switchToAppearance() {
      fireEvent.click(screen.getByTestId('settings-category-appearance'));
    }

    it('lists every registered theme plus a System option', () => {
      renderSettings();
      switchToAppearance();
      const group = screen.getByTestId('appearance-theme-options');
      // The picker enumerates the THEMES registry and appends System; this
      // assertion mirrors the registry order so a registry edit (the only
      // intended way to add a theme) is the single thing this expectation
      // needs to follow.
      const radios = within(group).getAllByRole('radio');
      expect(radios.map((r) => (r as HTMLInputElement).value)).toEqual([
        'dark',
        'light',
        'sepia',
        SYSTEM_PREFERENCE,
      ]);
      expect(within(group).getByText('Dark')).toBeInTheDocument();
      expect(within(group).getByText('Light')).toBeInTheDocument();
      expect(within(group).getByText('Sepia')).toBeInTheDocument();
      expect(within(group).getByText('System')).toBeInTheDocument();
    });

    it('highlights the current preference (defaults to System on a fresh install)', () => {
      renderSettings();
      switchToAppearance();
      const systemRadio = screen.getByTestId(
        `appearance-option-${SYSTEM_PREFERENCE}`,
      );
      expect(within(systemRadio).getByRole('radio')).toBeChecked();
      const darkRadio = screen.getByTestId('appearance-option-dark');
      expect(within(darkRadio).getByRole('radio')).not.toBeChecked();
    });

    it('highlights the stored preference rather than the resolved id', () => {
      // Under SYSTEM + prefers-dark = true, the resolved id is 'dark' but the
      // picker must still show System as the user's stated choice.
      installMatchMediaStub(true);
      localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, SYSTEM_PREFERENCE);
      renderSettings();
      switchToAppearance();
      const systemRadio = screen.getByTestId(
        `appearance-option-${SYSTEM_PREFERENCE}`,
      );
      expect(within(systemRadio).getByRole('radio')).toBeChecked();
    });

    it('writes data-theme + persists the pick when a theme is selected', () => {
      renderSettings();
      switchToAppearance();
      const darkRadio = within(
        screen.getByTestId('appearance-option-dark'),
      ).getByRole('radio');
      fireEvent.click(darkRadio);

      expect(darkRadio).toBeChecked();
      expect(document.documentElement.dataset.theme).toBe('dark');
      expect(localStorage.getItem(THEME_PREFERENCE_STORAGE_KEY)).toBe('dark');
    });

    it('switching back to System restores the OS-driven resolution', () => {
      // Pick light explicitly, then flip back to System; data-theme should
      // follow the matchMedia stub (light here) rather than stick on 'light'.
      renderSettings();
      switchToAppearance();
      fireEvent.click(
        within(screen.getByTestId('appearance-option-light')).getByRole('radio'),
      );
      expect(localStorage.getItem(THEME_PREFERENCE_STORAGE_KEY)).toBe('light');

      fireEvent.click(
        within(
          screen.getByTestId(`appearance-option-${SYSTEM_PREFERENCE}`),
        ).getByRole('radio'),
      );
      expect(localStorage.getItem(THEME_PREFERENCE_STORAGE_KEY)).toBe(
        SYSTEM_PREFERENCE,
      );
      expect(document.documentElement.dataset.theme).toBe('light');
    });

    it('exposes the three-way visual-effects control', () => {
      renderSettings();
      switchToAppearance();
      const group = screen.getByTestId('appearance-effects-options');
      const radios = within(group).getAllByRole('radio');
      expect(radios.map((r) => (r as HTMLInputElement).value)).toEqual([
        'auto',
        'on',
        'off',
      ]);
      expect(within(group).getByText('Auto (platform default)')).toBeInTheDocument();
    });

    it('reflects the stored visual-effects value (defaults to Auto)', () => {
      renderSettings();
      switchToAppearance();
      expect(
        within(screen.getByTestId('appearance-effects-option-auto')).getByRole(
          'radio',
        ),
      ).toBeChecked();
      expect(
        within(screen.getByTestId('appearance-effects-option-on')).getByRole(
          'radio',
        ),
      ).not.toBeChecked();
    });

    it('writes the store when a visual-effects option is picked', () => {
      renderSettings();
      switchToAppearance();
      const offRadio = within(
        screen.getByTestId('appearance-effects-option-off'),
      ).getByRole('radio');
      fireEvent.click(offRadio);

      expect(offRadio).toBeChecked();
      expect(useSettingsStore.getState().visualEffects).toBe('off');
    });
  });
});
