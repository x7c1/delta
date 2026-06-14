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
    useNavStore.setState({ settingsOpen: true });
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

  it('closes the settings screen via Close', async () => {
    renderSettings();
    await findList();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(useNavStore.getState().settingsOpen).toBe(false);
  });
});
