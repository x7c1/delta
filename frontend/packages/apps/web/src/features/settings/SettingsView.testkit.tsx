/**
 * Shared fixtures and helpers for the `SettingsView.*.test.tsx` family: the
 * MSW server, the render helper, the matchMedia stub, the finders for the
 * launch-option and prompt-template lists, and the per-case reset.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import { vi } from 'vitest';
import {
  fireEvent,
  render,
  screen,
  within,
  type RenderResult,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { ThemeProvider } from '../../hooks/themeContext';
import { THEME_PREFERENCE_STORAGE_KEY } from '../../hooks/useTheme';
import { useNavStore } from '../../store/navStore';
import {
  DEFAULT_SETTINGS_CATEGORY,
  DEFAULT_VISUAL_EFFECTS_SETTING,
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../../store/settingsStore';
import { SettingsView } from './SettingsView';
import { ErasedGate } from './storage/ErasedGate';

export const server = setupServer(...createHandlers());

export function renderSettings(): RenderResult {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        {/* The real app mounts ThemeProvider at the root (see App.tsx) so the
            settings picker and other consumers read the same theme state. */}
        <ThemeProvider>
          {/* As in App.tsx: the Erase everything category reports through it. */}
          <ErasedGate>
            <SettingsView />
          </ErasedGate>
        </ThemeProvider>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/** Stub `matchMedia` so the ThemeProvider can resolve its initial state in
 * jsdom (which does not implement the API). Tests can pass `true` to simulate
 * an OS that prefers dark mode. */
export function installMatchMediaStub(prefersDark: boolean) {
  const mql = {
    matches: prefersDark,
    media: '(prefers-color-scheme: dark)',
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  };
  vi.stubGlobal('matchMedia', () => mql);
}

/** The registered-options list once it has loaded its rows. */
export async function findList() {
  return screen.findByTestId('launch-options-list');
}

/** The full visible label of the add form's default-enabled checkbox. */
export const DEFAULT_ENABLED_LABEL =
  'Enabled by default (pre-checked when starting a session)';

/** The radio button for one provider in the section's provider selector. */
export function providerRadio(provider: 'claude' | 'codex') {
  return within(
    screen.getByTestId(`launch-option-provider-${provider}`),
  ).getByRole('radio');
}

/** Scope the launch-options section (form target + list) to one provider. */
export function selectProvider(provider: 'claude' | 'codex') {
  fireEvent.click(providerRadio(provider));
}

/** Switch the right pane to the Clone roots category. */
export function switchToCloneRoots() {
  fireEvent.click(screen.getByTestId('settings-category-clone-roots'));
}

/** Switch the right pane to the Prompt templates category. */
export function switchToPromptTemplates() {
  fireEvent.click(screen.getByTestId('settings-category-prompt-templates'));
}

/** The prompt-template list once it has loaded its rows. */
export async function findTemplateList() {
  return screen.findByTestId('prompt-templates-list');
}

/**
 * The body of the `Review checklist` fixture is several paragraphs long; this
 * line only appears in the middle of it, so finding it anywhere in the list
 * view means a preview leaked into a row.
 */
export const FIXTURE_BODY_MARKER = 'Check, in order:';

/** A multi-paragraph body, the shape a real template takes. */
export const MULTILINE_TEXT = [
  'Read the diff before touching anything.',
  '',
  'Then, in order:',
  '- correctness first;',
  '- then the tests.',
].join('\n');

/** Open the settings overlay on its defaults, with a light-OS matchMedia. */
export function resetSettingsState(): void {
  // The settings UI is a Dialog overlay gated on `settingsOpen`; open it so
  // the dialog (and its content) renders. The active category is persisted,
  // so reset it to the default between tests to keep them order-independent.
  useNavStore.setState({ settingsOpen: true });
  useSettingsStore.setState({
    activeCategory: DEFAULT_SETTINGS_CATEGORY,
    visualEffects: DEFAULT_VISUAL_EFFECTS_SETTING,
    defaultProvider: 'claude',
  });
  localStorage.removeItem(SETTINGS_STORAGE_KEY);
  // The ThemeProvider reads matchMedia + localStorage at mount; default to
  // light-OS + cleared preference so each test starts on the SYSTEM default.
  localStorage.removeItem(THEME_PREFERENCE_STORAGE_KEY);
  delete document.documentElement.dataset.theme;
  installMatchMediaStub(false);
}

/** Drop the globals a case stubbed. */
export function unstubGlobals(): void {
  vi.unstubAllGlobals();
}
