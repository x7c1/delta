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

describe('SettingsView', () => {
  beforeEach(() => {
    // The settings UI is a Dialog overlay gated on `settingsOpen`; open it so
    // the dialog (and its content) renders.
    useNavStore.setState({ settingsOpen: true });
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

  describe('Repository scan roots section', () => {
    it('renders the empty-state when no roots are registered', async () => {
      renderSettings();
      const section = await screen.findByTestId('scan-roots-section');
      expect(
        within(section).getByText('No scan roots registered yet.'),
      ).toBeInTheDocument();
      expect(within(section).queryByTestId('scan-roots-list')).toBeNull();
    });

    it('adds a scan root through the picker dialog', async () => {
      renderSettings();
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
