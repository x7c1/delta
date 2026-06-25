import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
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
import { useNavStore } from '../../store/navStore';
import {
  DEFAULT_SETTINGS_CATEGORY,
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
        <SettingsView />
      </ApiProvider>
    </QueryClientProvider>,
  );
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
    useSettingsStore.setState({ activeCategory: DEFAULT_SETTINGS_CATEGORY });
    localStorage.removeItem(SETTINGS_STORAGE_KEY);
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
      ]);
      expect(
        screen.getByTestId('settings-category-launch-options'),
      ).toHaveAttribute('aria-selected', 'true');
      expect(
        screen.getByTestId('settings-category-scan-roots'),
      ).toHaveAttribute('aria-selected', 'false');
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
