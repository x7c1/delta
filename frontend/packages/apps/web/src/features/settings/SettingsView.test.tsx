import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { ThemeProvider } from '../../hooks/themeContext';
import {
  SYSTEM_PREFERENCE,
  THEME_PREFERENCE_STORAGE_KEY,
} from '../../hooks/useTheme';
import { useNavStore } from '../../store/navStore';
import {
  DEFAULT_SETTINGS_CATEGORY,
  DEFAULT_VISUAL_EFFECTS_SETTING,
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../../store/settingsStore';
import { SettingsView } from './SettingsView';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderSettings() {
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
          <SettingsView />
        </ThemeProvider>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/** Stub `matchMedia` so the ThemeProvider can resolve its initial state in
 * jsdom (which does not implement the API). Tests can pass `true` to simulate
 * an OS that prefers dark mode. */
function installMatchMediaStub(prefersDark: boolean) {
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
async function findList() {
  return screen.findByTestId('launch-options-list');
}

/** Switch the right pane to the Repository scan roots category. */
function switchToScanRoots() {
  fireEvent.click(screen.getByTestId('settings-category-scan-roots'));
}

describe('SettingsView', () => {
  beforeEach(() => {
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
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders nothing while the settings overlay is closed', () => {
    useNavStore.setState({ settingsOpen: false });
    renderSettings();
    expect(screen.queryByTestId('dialog-backdrop')).toBeNull();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('renders the launch options inside the dialog overlay', async () => {
    renderSettings();
    await findList();
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByTestId('launch-options-list')).toBeInTheDocument();
  });

  it('lists the seeded launch options', async () => {
    renderSettings();
    const list = await findList();
    expect(within(list).getByText('--permission-mode')).toBeInTheDocument();
    expect(within(list).getByText('--plugin-dir')).toBeInTheDocument();
  });

  it('adds a launch option through the form', async () => {
    renderSettings();
    const list = await findList();
    // Sanity: the new flag is not present before submission.
    expect(within(list).queryByText('--model')).toBeNull();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '--model' },
    });
    fireEvent.change(screen.getByLabelText('Value (optional)'), {
      target: { value: 'opus' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    await waitFor(() =>
      expect(within(list).getByText('--model')).toBeInTheDocument(),
    );
  });

  it('names the provider on each registered option', async () => {
    renderSettings();
    const list = await findList();
    // The Codex fixture (`model gpt-5`) carries the written Codex name; the
    // Claude fixtures carry the written Claude Code name (each hue-tinted by
    // ProviderName).
    expect(within(list).getByText('model')).toBeInTheDocument();
    expect(within(list).getAllByText('Codex').length).toBeGreaterThan(0);
    expect(within(list).getAllByText('Claude Code').length).toBeGreaterThan(0);
  });

  it('registers a launch option for the selected provider', async () => {
    renderSettings();
    const list = await findList();

    // Pick Codex in the create form's provider selector, then add an option.
    fireEvent.click(
      within(
        screen.getByTestId('launch-option-provider-codex'),
      ).getByRole('radio'),
    );
    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: 'reasoning-effort' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    // The new option appears; its row carries the written, hue-tinted Codex
    // name, proving the chosen provider was sent and round-tripped.
    const row = await within(list).findByText('reasoning-effort');
    const li = row.closest('li');
    expect(li).not.toBeNull();
    expect(
      within(li as HTMLElement).getByText('Codex').className,
    ).toContain('text-provider-codex');
  });

  it('disables Add until a non-blank name is entered', async () => {
    renderSettings();
    await findList();
    const addButton = screen.getByRole('button', { name: 'Add option' });
    expect(addButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '   ' },
    });
    expect(addButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '--verbose' },
    });
    expect(addButton).toBeEnabled();
  });

  it('toggles a launch option default_enabled flag and persists it', async () => {
    renderSettings();
    const list = await findList();
    // `--permission-mode` (id 2) starts off; `--plugin-dir` (id 1) starts on.
    const permissionModeToggle = within(list).getByRole('checkbox', {
      name: 'Enable launch option --permission-mode by default',
    });
    const pluginDirToggle = within(list).getByRole('checkbox', {
      name: 'Enable launch option --plugin-dir by default',
    });
    expect(permissionModeToggle).not.toBeChecked();
    expect(pluginDirToggle).toBeChecked();

    // Toggling it on round-trips through the mock store (the list refetches).
    fireEvent.click(permissionModeToggle);
    await waitFor(() =>
      expect(
        within(list).getByRole('checkbox', {
          name: 'Enable launch option --permission-mode by default',
        }),
      ).toBeChecked(),
    );
  });

  it('deletes a launch option', async () => {
    renderSettings();
    const list = await findList();
    const target = within(list).getByText('--permission-mode');

    fireEvent.click(
      within(list).getByRole('button', {
        name: 'Delete launch option --permission-mode',
      }),
    );

    await waitFor(() => expect(target).not.toBeInTheDocument());
    expect(within(list).getByText('--plugin-dir')).toBeInTheDocument();
  });

  it('closes the settings overlay via Close', async () => {
    renderSettings();
    await findList();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(useNavStore.getState().settingsOpen).toBe(false);
  });

  describe('category sidebar', () => {
    it('opens on the persisted category and reveals its right pane', () => {
      useSettingsStore.setState({ activeCategory: 'scan-roots' });
      renderSettings();
      expect(screen.getByTestId('settings-category-scan-roots')).toHaveAttribute(
        'aria-selected',
        'true',
      );
      expect(screen.getByTestId('scan-roots-section')).toBeInTheDocument();
      expect(screen.queryByTestId('launch-options-section')).toBeNull();
    });

    it('switches the right pane content when a category is clicked', async () => {
      renderSettings();
      // Default landing pane is Launch options; the scan-roots pane is not
      // mounted yet.
      expect(screen.getByTestId('launch-options-section')).toBeInTheDocument();
      expect(screen.queryByTestId('scan-roots-section')).toBeNull();

      switchToScanRoots();

      expect(screen.queryByTestId('launch-options-section')).toBeNull();
      expect(await screen.findByTestId('scan-roots-section')).toBeInTheDocument();
    });

    it('persists the active category to localStorage', () => {
      renderSettings();
      switchToScanRoots();
      const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
      expect(raw).not.toBeNull();
      // The persist middleware wraps state under `{ state, version }`; assert
      // on the parsed shape rather than substring-matching the JSON.
      const parsed = JSON.parse(raw ?? '{}');
      expect(parsed.state.activeCategory).toBe('scan-roots');
    });

    it('exposes a vertical tablist with one tab per category', () => {
      renderSettings();
      const tablist = screen.getByRole('tablist');
      expect(tablist).toHaveAttribute('aria-orientation', 'vertical');
      const tabs = within(tablist).getAllByRole('tab');
      expect(tabs.map((t) => t.textContent)).toEqual([
        'Launch options',
        'Repository scan roots',
        'Appearance',
        'Default provider',
      ]);
      expect(
        screen.getByTestId('settings-category-launch-options'),
      ).toHaveAttribute('aria-selected', 'true');
      expect(
        screen.getByTestId('settings-category-scan-roots'),
      ).toHaveAttribute('aria-selected', 'false');
      expect(
        screen.getByTestId('settings-category-appearance'),
      ).toHaveAttribute('aria-selected', 'false');
      expect(
        screen.getByTestId('settings-category-default-provider'),
      ).toHaveAttribute('aria-selected', 'false');
    });

  });

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

  describe('Repository scan roots section', () => {
    it('renders the empty-state when no roots are registered', async () => {
      renderSettings();
      switchToScanRoots();
      const section = await screen.findByTestId('scan-roots-section');
      expect(
        within(section).getByText('No scan roots registered yet.'),
      ).toBeInTheDocument();
      expect(within(section).queryByTestId('scan-roots-list')).toBeNull();
    });

    it('adds a scan root through the picker dialog', async () => {
      renderSettings();
      switchToScanRoots();
      const section = await screen.findByTestId('scan-roots-section');
      fireEvent.click(within(section).getByTestId('add-scan-root'));

      // The picker dialog opens; the WorkdirPickerBody pre-selects the most
      // recent directory as its candidate, so Add becomes enabled without
      // needing to click a tree node first.
      const confirm = await screen.findByTestId('scan-root-confirm');
      await waitFor(() => expect(confirm).toBeEnabled());
      fireEvent.click(confirm);

      // After success the picker closes and the list shows the new entry.
      await waitFor(() =>
        expect(within(section).getByTestId('scan-roots-list')).toBeInTheDocument(),
      );
    });

    it('shows an inline duplicate hint when adding the same root twice', async () => {
      renderSettings();
      switchToScanRoots();
      const section = await screen.findByTestId('scan-roots-section');

      // First registration: the picker opens, the candidate is auto-pre-
      // selected (the WorkdirPickerBody seeds it from the recent list), and
      // submitting succeeds.
      fireEvent.click(within(section).getByTestId('add-scan-root'));
      const firstConfirm = await screen.findByTestId('scan-root-confirm');
      await waitFor(() => expect(firstConfirm).toBeEnabled());
      fireEvent.click(firstConfirm);
      await waitFor(() =>
        expect(within(section).getByTestId('scan-roots-list')).toBeInTheDocument(),
      );

      // Second attempt at the same path: the server replies 409 with the
      // stable code, and the picker shows an inline "Already registered" hint
      // instead of a global toast.
      fireEvent.click(within(section).getByTestId('add-scan-root'));
      const secondConfirm = await screen.findByTestId('scan-root-confirm');
      await waitFor(() => expect(secondConfirm).toBeEnabled());
      fireEvent.click(secondConfirm);
      await waitFor(() =>
        expect(screen.getByTestId('scan-root-duplicate')).toBeInTheDocument(),
      );
    });

    it('removes a registered scan root', async () => {
      renderSettings();
      switchToScanRoots();
      const section = await screen.findByTestId('scan-roots-section');

      // Register one first so there is a row to remove.
      fireEvent.click(within(section).getByTestId('add-scan-root'));
      const confirm = await screen.findByTestId('scan-root-confirm');
      await waitFor(() => expect(confirm).toBeEnabled());
      fireEvent.click(confirm);
      const list = await within(section).findByTestId('scan-roots-list');

      // Click the row's Remove button (the only button inside the list row),
      // and the row disappears as the list refetches.
      const removeButton = within(list).getByRole('button', {
        name: /Remove scan root /,
      });
      fireEvent.click(removeButton);
      await waitFor(() =>
        expect(within(section).queryByTestId('scan-roots-list')).toBeNull(),
      );
      expect(
        within(section).getByText('No scan roots registered yet.'),
      ).toBeInTheDocument();
    });
  });
});
