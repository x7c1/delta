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
});
