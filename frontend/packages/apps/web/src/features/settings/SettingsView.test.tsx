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
import { http, HttpResponse } from 'msw';
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
import type { SettingsCategoryId } from '../../store/settingsStore';
import {
  DEFAULT_SETTINGS_CATEGORY,
  DEFAULT_VISUAL_EFFECTS_SETTING,
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../../store/settingsStore';
import { SettingsView } from './SettingsView';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
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

/** The full visible label of the add form's default-enabled checkbox. */
const DEFAULT_ENABLED_LABEL =
  'Enabled by default (pre-checked when starting a session)';

/** The radio button for one provider in the section's provider selector. */
function providerRadio(provider: 'claude' | 'codex') {
  return within(
    screen.getByTestId(`launch-option-provider-${provider}`),
  ).getByRole('radio');
}

/** Scope the launch-options section (form target + list) to one provider. */
function selectProvider(provider: 'claude' | 'codex') {
  fireEvent.click(providerRadio(provider));
}

/** Switch the right pane to the Clone roots category. */
function switchToCloneRoots() {
  fireEvent.click(screen.getByTestId('settings-category-clone-roots'));
}

/** Switch the right pane to the Prompt templates category. */
function switchToPromptTemplates() {
  fireEvent.click(screen.getByTestId('settings-category-prompt-templates'));
}

/** The prompt-template list once it has loaded its rows. */
async function findTemplateList() {
  return screen.findByTestId('prompt-templates-list');
}

/**
 * The body of the `Review checklist` fixture is several paragraphs long; this
 * line only appears in the middle of it, so finding it anywhere in the list
 * view means a preview leaked into a row.
 */
const FIXTURE_BODY_MARKER = 'Check, in order:';

/** A multi-paragraph body, the shape a real template takes. */
const MULTILINE_TEXT = [
  'Read the diff before touching anything.',
  '',
  'Then, in order:',
  '- correctness first;',
  '- then the tests.',
].join('\n');

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

  it('lists the seeded launch options of the selected provider', async () => {
    renderSettings();
    const list = await findList();
    // The section opens on the configured default provider (Claude, per the
    // beforeEach), so only the two Claude fixtures are listed — the Codex one
    // (`model`) is filtered out.
    expect(within(list).getByText('--permission-mode')).toBeInTheDocument();
    expect(within(list).getByText('--plugin-dir')).toBeInTheDocument();
    expect(within(list).queryByText('model')).toBeNull();
  });

  it('opens scoped to the configured default provider', async () => {
    // A Codex-first user (Settings → Default provider = Codex) must land on
    // their own options, not on Claude's list.
    useSettingsStore.setState({ defaultProvider: 'codex' });
    renderSettings();
    const list = await findList();
    expect(providerRadio('codex')).toBeChecked();
    expect(within(list).getByText('model')).toBeInTheDocument();
    expect(within(list).queryByText('--permission-mode')).toBeNull();
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

  it('omits the provider name from the option rows', async () => {
    renderSettings();
    const list = await findList();
    expect(within(list).queryByText('Claude Code')).toBeNull();
    expect(within(list).queryByText('Codex')).toBeNull();
  });

  it('swaps the listed options when the provider selector changes', async () => {
    renderSettings();
    await findList();

    selectProvider('codex');

    // Only the Codex fixture (`model gpt-5`) remains listed.
    const codexList = await findList();
    expect(within(codexList).getByText('model')).toBeInTheDocument();
    expect(within(codexList).queryByText('--permission-mode')).toBeNull();
    expect(within(codexList).queryByText('--plugin-dir')).toBeNull();

    // And back: the Claude options return.
    selectProvider('claude');
    const claudeList = await findList();
    expect(within(claudeList).getByText('--permission-mode')).toBeInTheDocument();
    expect(within(claudeList).queryByText('model')).toBeNull();
  });

  it('shows a provider-scoped empty state when only the other provider has options', async () => {
    // A registry holding Claude options only: on Codex the list must not claim
    // the whole registry is empty, nor leak the Claude rows.
    server.use(
      http.get('*/api/launch-options', () =>
        HttpResponse.json({
          launch_options: [
            {
              id: 1,
              label: 'My plugins',
              name: '--plugin-dir',
              value: '/home/dev/plugins',
              default_enabled: true,
              created_at: '2026-01-01T00:00:00Z',
              provider: 'claude',
            },
          ],
        }),
      ),
    );
    renderSettings();
    await findList();

    selectProvider('codex');

    const empty = await screen.findByTestId('launch-options-empty');
    expect(empty.textContent).toBe('No launch options registered for Codex yet.');
    expect(screen.queryByTestId('launch-options-list')).toBeNull();
  });

  it('registers a launch option for the selected provider and stays scoped to it', async () => {
    renderSettings();
    await findList();

    // Pick Codex in the section's provider selector, then add an option. Codex
    // takes field-style options, so the name input is labelled for a field.
    selectProvider('codex');
    fireEvent.change(screen.getByLabelText('Label (optional)'), {
      target: { value: 'Reasoning' },
    });
    fireEvent.change(screen.getByLabelText('Name (the field)'), {
      target: { value: 'reasoning-effort' },
    });
    fireEvent.change(screen.getByLabelText('Value (optional)'), {
      target: { value: 'high' },
    });
    fireEvent.click(screen.getByLabelText(DEFAULT_ENABLED_LABEL));
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    // The new option appears in the still-Codex-scoped list, proving the chosen
    // provider was sent and round-tripped.
    const list = await findList();
    await within(list).findByText('reasoning-effort');
    expect(within(list).queryByText('--permission-mode')).toBeNull();

    // The selector stays on Codex while the other fields are cleared.
    expect(providerRadio('codex')).toBeChecked();
    expect(providerRadio('claude')).not.toBeChecked();
    expect(screen.getByLabelText('Label (optional)')).toHaveValue('');
    expect(screen.getByLabelText('Name (the field)')).toHaveValue('');
    expect(screen.getByLabelText('Value (optional)')).toHaveValue('');
    expect(screen.getByLabelText(DEFAULT_ENABLED_LABEL)).not.toBeChecked();
  });

  it('words the add form for the selected provider launch-option style', async () => {
    renderSettings();
    await findList();

    // Claude takes CLI flags: the name input is labelled and exemplified as one.
    const flagName = screen.getByLabelText('Name (the flag)');
    expect(flagName).toHaveAttribute('placeholder', '--permission-mode');
    expect(screen.getByLabelText('Value (optional)')).toHaveAttribute(
      'placeholder',
      'auto',
    );
    expect(screen.getByTestId('launch-options-section').textContent).toContain(
      'Register custom CLI flags',
    );

    selectProvider('codex');

    // Codex takes session-start request fields: a user must write `model`, not
    // `--model`, so nothing in the form may still say "flag".
    await waitFor(() =>
      expect(screen.queryByLabelText('Name (the flag)')).toBeNull(),
    );
    expect(screen.getByLabelText('Name (the field)')).toHaveAttribute(
      'placeholder',
      'model',
    );
    expect(screen.getByLabelText('Value (optional)')).toHaveAttribute(
      'placeholder',
      'gpt-5.6-sol',
    );
    const section = screen.getByTestId('launch-options-section');
    expect(section.textContent).toContain('Register custom session-start settings');
    expect(section.textContent).not.toContain('CLI flags');
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
      useSettingsStore.setState({ activeCategory: 'clone-roots' });
      renderSettings();
      expect(screen.getByTestId('settings-category-clone-roots')).toHaveAttribute(
        'aria-selected',
        'true',
      );
      expect(screen.getByTestId('clone-roots-section')).toBeInTheDocument();
      expect(screen.queryByTestId('launch-options-section')).toBeNull();
    });

    it('renders the first category when the active id is not in the registry', () => {
      // A category id an earlier build persisted under a name this one has
      // since renamed (or any other value the registry does not know) reaches
      // the view only if it slips past the store's rehydration guard — a
      // `setState` from stale code, a future id, a hand-edited localStorage.
      // The rail-to-pane lookup must still land on a real category rather than
      // leaving the right pane blank with every rail entry unselected.
      useSettingsStore.setState({
        activeCategory: 'retired-category' as SettingsCategoryId,
      });
      renderSettings();
      expect(screen.getByTestId('launch-options-section')).toBeInTheDocument();
      expect(screen.queryByTestId('clone-roots-section')).toBeNull();
      // The rail follows the same fallback, so the rendered pane still has its
      // tab marked selected instead of the rail showing no selection at all.
      expect(
        screen.getByTestId('settings-category-launch-options'),
      ).toHaveAttribute('aria-selected', 'true');
    });

    it('switches the right pane content when a category is clicked', async () => {
      renderSettings();
      // Default landing pane is Launch options; the clone-roots pane is not
      // mounted yet.
      expect(screen.getByTestId('launch-options-section')).toBeInTheDocument();
      expect(screen.queryByTestId('clone-roots-section')).toBeNull();

      switchToCloneRoots();

      expect(screen.queryByTestId('launch-options-section')).toBeNull();
      expect(await screen.findByTestId('clone-roots-section')).toBeInTheDocument();
    });

    it('persists the active category to localStorage', () => {
      renderSettings();
      switchToCloneRoots();
      const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
      expect(raw).not.toBeNull();
      // The persist middleware wraps state under `{ state, version }`; assert
      // on the parsed shape rather than substring-matching the JSON.
      const parsed = JSON.parse(raw ?? '{}');
      expect(parsed.state.activeCategory).toBe('clone-roots');
    });

    it('exposes a vertical tablist with one tab per category', () => {
      renderSettings();
      const tablist = screen.getByRole('tablist');
      expect(tablist).toHaveAttribute('aria-orientation', 'vertical');
      const tabs = within(tablist).getAllByRole('tab');
      expect(tabs.map((t) => t.textContent)).toEqual([
        'Launch options',
        'Prompt templates',
        'Clone roots',
        'Appearance',
        'Default provider',
      ]);
      expect(
        screen.getByTestId('settings-category-launch-options'),
      ).toHaveAttribute('aria-selected', 'true');
      expect(
        screen.getByTestId('settings-category-prompt-templates'),
      ).toHaveAttribute('aria-selected', 'false');
      expect(
        screen.getByTestId('settings-category-clone-roots'),
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

  describe('Clone roots section', () => {
    it('renders the empty-state when no roots are registered', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');
      expect(
        within(section).getByText('No clone roots registered yet.'),
      ).toBeInTheDocument();
      expect(within(section).queryByTestId('clone-roots-list')).toBeNull();
    });

    it('adds a clone root through the picker dialog', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');
      fireEvent.click(within(section).getByTestId('add-clone-root'));

      // The picker dialog opens; the WorkdirPickerBody pre-selects the most
      // recent directory as its candidate, so Add becomes enabled without
      // needing to click a tree node first.
      const confirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(confirm).toBeEnabled());
      fireEvent.click(confirm);

      // After success the picker closes and the list shows the new entry.
      await waitFor(() =>
        expect(within(section).getByTestId('clone-roots-list')).toBeInTheDocument(),
      );
    });

    it('shows an inline duplicate hint when adding the same root twice', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');

      // First registration: the picker opens, the candidate is auto-pre-
      // selected (the WorkdirPickerBody seeds it from the recent list), and
      // submitting succeeds.
      fireEvent.click(within(section).getByTestId('add-clone-root'));
      const firstConfirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(firstConfirm).toBeEnabled());
      fireEvent.click(firstConfirm);
      await waitFor(() =>
        expect(within(section).getByTestId('clone-roots-list')).toBeInTheDocument(),
      );

      // Second attempt at the same path: the server replies 409 with the
      // stable code, and the picker shows an inline "Already registered" hint
      // instead of a global toast.
      fireEvent.click(within(section).getByTestId('add-clone-root'));
      const secondConfirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(secondConfirm).toBeEnabled());
      fireEvent.click(secondConfirm);
      await waitFor(() =>
        expect(screen.getByTestId('clone-root-duplicate')).toBeInTheDocument(),
      );
    });

    it('removes a registered clone root', async () => {
      renderSettings();
      switchToCloneRoots();
      const section = await screen.findByTestId('clone-roots-section');

      // Register one first so there is a row to remove.
      fireEvent.click(within(section).getByTestId('add-clone-root'));
      const confirm = await screen.findByTestId('clone-root-confirm');
      await waitFor(() => expect(confirm).toBeEnabled());
      fireEvent.click(confirm);
      const list = await within(section).findByTestId('clone-roots-list');

      // Click the row's Remove button (the only button inside the list row),
      // and the row disappears as the list refetches.
      const removeButton = within(list).getByRole('button', {
        name: /Remove clone root /,
      });
      fireEvent.click(removeButton);
      await waitFor(() =>
        expect(within(section).queryByTestId('clone-roots-list')).toBeNull(),
      );
      expect(
        within(section).getByText('No clone roots registered yet.'),
      ).toBeInTheDocument();
    });
  });

  describe('Prompt templates section', () => {
    it('renders only the prompt-templates pane and persists the selection', async () => {
      renderSettings();
      switchToPromptTemplates();

      expect(
        await screen.findByTestId('prompt-templates-section'),
      ).toBeInTheDocument();
      expect(screen.queryByTestId('launch-options-section')).toBeNull();
      expect(screen.queryByTestId('clone-roots-section')).toBeNull();
      expect(
        screen.getByTestId('settings-category-prompt-templates'),
      ).toHaveAttribute('aria-selected', 'true');
      expect(useSettingsStore.getState().activeCategory).toBe(
        'prompt-templates',
      );
      const parsed = JSON.parse(
        localStorage.getItem(SETTINGS_STORAGE_KEY) ?? '{}',
      );
      expect(parsed.state.activeCategory).toBe('prompt-templates');
    });

    it('lists one row per template, showing the label and none of the body', async () => {
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      // The API's own order is the list's order: oldest first, so the fixture
      // created on 2026-01-01 precedes the one created on 2026-01-02. The pane
      // must not re-sort (by label, by `updated_at`, or otherwise) — a
      // template's position is the one the user will learn to reach for.
      const rows = within(list).getAllByRole('listitem');
      expect(rows).toHaveLength(2);
      expect(rows[0]).toHaveTextContent('Merge when green');
      expect(rows[1]).toHaveTextContent('Review checklist');

      // A template body runs to paragraphs, so no part of either fixture's
      // text may reach the pane — not even a truncated first line.
      const section = screen.getByTestId('prompt-templates-section');
      expect(section.textContent).not.toContain(FIXTURE_BODY_MARKER);
      expect(section.textContent).not.toContain('Once CI is green');

      for (const label of ['Merge when green', 'Review checklist']) {
        expect(
          within(list).getByRole('button', {
            name: `Edit prompt template ${label}`,
          }),
        ).toBeInTheDocument();
        expect(
          within(list).getByRole('button', {
            name: `Delete prompt template ${label}`,
          }),
        ).toBeInTheDocument();
      }
    });

    it('shows the empty state when nothing is registered', async () => {
      server.use(
        http.get('*/api/prompt-templates', () =>
          HttpResponse.json({ prompt_templates: [] }),
        ),
      );
      renderSettings();
      switchToPromptTemplates();

      const empty = await screen.findByTestId('prompt-templates-empty');
      expect(empty.textContent).toBe('No prompt templates yet.');
      expect(screen.queryByTestId('prompt-templates-list')).toBeNull();
    });

    it('opens an empty editor from New template and gates Save on both fields', async () => {
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();

      fireEvent.click(screen.getByTestId('prompt-template-new'));

      // The editor replaces the list inside the same pane.
      expect(screen.getByTestId('prompt-template-editor')).toBeInTheDocument();
      expect(screen.queryByTestId('prompt-templates-list')).toBeNull();
      expect(screen.queryByTestId('prompt-template-new')).toBeNull();

      const label = screen.getByLabelText('Label');
      const text = screen.getByLabelText('Text');
      expect(label).toHaveValue('');
      expect(text).toHaveValue('');
      const save = screen.getByRole('button', { name: 'Save' });
      expect(save).toBeDisabled();

      // Whitespace is not content: the server rejects a blank field, so the
      // button must not promise otherwise.
      fireEvent.change(label, { target: { value: '   ' } });
      fireEvent.change(text, { target: { value: MULTILINE_TEXT } });
      expect(save).toBeDisabled();

      fireEvent.change(label, { target: { value: 'Triage' } });
      expect(save).toBeEnabled();
    });

    it('creates a template, sending the text verbatim, and returns to the list', async () => {
      const posted: unknown[] = [];
      server.use(
        http.post('*/api/prompt-templates', async ({ request }) => {
          posted.push(await request.clone().json());
          // Returning nothing hands the request to the default mock handler,
          // so the create still round-trips through the mock store and the
          // refreshed list shows the new row.
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();

      fireEvent.click(screen.getByTestId('prompt-template-new'));
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Triage' },
      });
      fireEvent.change(screen.getByLabelText('Text'), {
        target: { value: MULTILINE_TEXT },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      const list = await findTemplateList();
      await within(list).findByText('Triage');
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
      // Newlines and blank lines survive: the body is never trimmed or
      // collapsed on its way to the server.
      expect(posted).toEqual([{ label: 'Triage', text: MULTILINE_TEXT }]);
    });

    it('edits a template from its row and patches the label and text', async () => {
      const patched: { id: string; body: unknown }[] = [];
      server.use(
        http.patch('*/api/prompt-templates/:id', async ({ params, request }) => {
          patched.push({
            id: String(params.id),
            body: await request.clone().json(),
          });
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Edit prompt template Review checklist',
        }),
      );

      // Pre-filled with the row's own content — the whole body, not a preview.
      expect(screen.getByLabelText('Label')).toHaveValue('Review checklist');
      const text = screen.getByLabelText('Text') as HTMLTextAreaElement;
      expect(text.value).toContain(FIXTURE_BODY_MARKER);
      expect(text.value.split('\n').length).toBeGreaterThan(1);
      const originalText = text.value;

      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Review checklist v2' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      // The list is re-queried: the editor unmounted the previous `ul`, so the
      // one showing now is a different node.
      const updated = await findTemplateList();
      await within(updated).findByText('Review checklist v2');
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
      expect(patched).toEqual([
        {
          // The seeded `Review checklist` fixture.
          id: '2',
          body: { label: 'Review checklist v2', text: originalText },
        },
      ]);
    });

    it('cancels the editor without a request and without changing the list', async () => {
      const writes: string[] = [];
      server.use(
        http.post('*/api/prompt-templates', () => {
          writes.push('POST');
          return HttpResponse.json({ error: 'unexpected' }, { status: 500 });
        }),
        http.patch('*/api/prompt-templates/:id', () => {
          writes.push('PATCH');
          return HttpResponse.json({ error: 'unexpected' }, { status: 500 });
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Edit prompt template Merge when green',
        }),
      );
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Discarded' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      const back = await findTemplateList();
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
      expect(within(back).getByText('Merge when green')).toBeInTheDocument();
      expect(within(back).queryByText('Discarded')).toBeNull();
      expect(writes).toEqual([]);

      // Reopening starts from the stored content, so the abandoned edit is
      // genuinely gone rather than parked.
      fireEvent.click(
        within(back).getByRole('button', {
          name: 'Edit prompt template Merge when green',
        }),
      );
      expect(screen.getByLabelText('Label')).toHaveValue('Merge when green');
    });

    it('discards an open draft when the category is left', async () => {
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();
      fireEvent.click(screen.getByTestId('prompt-template-new'));
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Half-written' },
      });

      switchToCloneRoots();
      await screen.findByTestId('clone-roots-section');
      switchToPromptTemplates();

      // Back on the list, not on the half-written draft.
      await findTemplateList();
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
    });

    it('confirms a delete by name and removes the row', async () => {
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Delete prompt template Merge when green',
        }),
      );

      // The confirmation names the template it is about to destroy.
      const confirmation = screen
        .getByText('Delete prompt template')
        .closest('[role="dialog"]');
      expect(confirmation).not.toBeNull();
      expect(confirmation?.textContent).toContain('Merge when green');

      fireEvent.click(screen.getByTestId('prompt-template-delete-confirm'));

      await waitFor(() =>
        expect(within(list).queryByText('Merge when green')).toBeNull(),
      );
      expect(within(list).getByText('Review checklist')).toBeInTheDocument();
      // The confirmation closes itself once the delete lands.
      expect(screen.queryByTestId('prompt-template-delete-confirm')).toBeNull();
    });

    it('issues no request when the delete confirmation is dismissed', async () => {
      const deletes: string[] = [];
      server.use(
        http.delete('*/api/prompt-templates/:id', ({ params }) => {
          deletes.push(String(params.id));
          return new HttpResponse(null, { status: 204 });
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Delete prompt template Merge when green',
        }),
      );
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(screen.queryByTestId('prompt-template-delete-confirm')).toBeNull();
      expect(within(list).getByText('Merge when green')).toBeInTheDocument();
      expect(deletes).toEqual([]);
    });

    it('surfaces a failed save inline and keeps the editor content', async () => {
      server.use(
        http.post('*/api/prompt-templates', () =>
          HttpResponse.json(
            { error: 'a prompt template must have a non-blank `label`' },
            { status: 400 },
          ),
        ),
      );
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();

      fireEvent.click(screen.getByTestId('prompt-template-new'));
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Triage' },
      });
      fireEvent.change(screen.getByLabelText('Text'), {
        target: { value: MULTILINE_TEXT },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      const error = await screen.findByTestId('prompt-template-save-error');
      expect(error.textContent).toContain('Could not save the prompt template.');
      // The server's own reason is repeated rather than swallowed.
      expect(error.textContent).toContain('non-blank');
      // The draft survives so the user can fix it and retry.
      expect(screen.getByTestId('prompt-template-editor')).toBeInTheDocument();
      expect(screen.getByLabelText('Label')).toHaveValue('Triage');
      expect(screen.getByLabelText('Text')).toHaveValue(MULTILINE_TEXT);
    });

    it('surfaces a failed delete and keeps the row', async () => {
      server.use(
        http.delete('*/api/prompt-templates/:id', () =>
          HttpResponse.json({ error: 'database is locked' }, { status: 500 }),
        ),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Delete prompt template Merge when green',
        }),
      );
      fireEvent.click(screen.getByTestId('prompt-template-delete-confirm'));

      const error = await screen.findByTestId('prompt-template-delete-error');
      expect(error.textContent).toContain(
        'Could not delete the prompt template.',
      );
      expect(error.textContent).toContain('database is locked');
      expect(within(list).getByText('Merge when green')).toBeInTheDocument();
    });
  });
});
