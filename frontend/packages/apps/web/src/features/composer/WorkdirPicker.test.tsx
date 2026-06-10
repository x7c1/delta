import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { WorkdirPicker } from './WorkdirPicker';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPicker() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <WorkdirPicker />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('WorkdirPicker', () => {
  beforeEach(() => {
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it('selects a directory from the Recent list', async () => {
    renderPicker();

    // The mock recent list includes the delta project as a clickable row.
    const recentRow = await screen.findByRole('button', {
      name: '/home/dev/projects/delta',
    });
    fireEvent.click(recentRow);

    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
  });

  it('descends into and ascends out of a directory while browsing', async () => {
    renderPicker();

    // Default browse lists $HOME (/home/dev) with its subdirectories.
    await waitFor(() => {
      expect(screen.getByTestId('workdir-current-path')).toHaveTextContent(
        '/home/dev',
      );
    });
    expect(
      screen.getByRole('button', { name: 'projects/' }),
    ).toBeInTheDocument();

    // Descend into projects/.
    fireEvent.click(screen.getByRole('button', { name: 'projects/' }));
    await waitFor(() => {
      expect(screen.getByTestId('workdir-current-path')).toHaveTextContent(
        '/home/dev/projects',
      );
    });
    expect(screen.getByRole('button', { name: 'delta/' })).toBeInTheDocument();

    // Ascend via the ".." entry back to $HOME.
    fireEvent.click(screen.getByTestId('workdir-parent'));
    await waitFor(() => {
      expect(screen.getByTestId('workdir-current-path')).toHaveTextContent(
        '/home/dev',
      );
    });
  });

  it('selects the currently-browsed directory as the cwd', async () => {
    renderPicker();

    await waitFor(() => {
      expect(screen.getByTestId('workdir-current-path')).toHaveTextContent(
        '/home/dev',
      );
    });
    fireEvent.click(screen.getByTestId('workdir-select-current'));

    expect(useComposerStore.getState().newSessionWorkdir).toBe('/home/dev');
  });

  it('shows an inline error and offers a way back when a listing is forbidden', async () => {
    // Force the default ($HOME) listing to 403 so the error path renders without
    // first needing a successful listing to know the parent.
    server.use(
      http.get('*/api/workdir/list', () =>
        HttpResponse.json({ error: 'permission denied' }, { status: 403 }),
      ),
    );
    renderPicker();

    const error = await screen.findByTestId('workdir-error');
    expect(error).toHaveTextContent('Permission denied');
    // With no prior successful listing the recovery falls back to home.
    expect(
      screen.getByRole('button', { name: 'Back to home' }),
    ).toBeInTheDocument();
  });

  it('shows an inline error for a 400 (missing/non-directory) listing', async () => {
    server.use(
      http.get('*/api/workdir/list', () =>
        HttpResponse.json({ error: 'not a directory' }, { status: 400 }),
      ),
    );
    renderPicker();

    const error = await screen.findByTestId('workdir-error');
    expect(error).toHaveTextContent('could not be opened');
  });

  it('hides the Recent section when the recent query fails', async () => {
    server.use(
      http.get('*/api/workdir/recent', () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 }),
      ),
    );
    renderPicker();

    // Browse still renders; Recent is omitted entirely on failure.
    await waitFor(() => {
      expect(screen.getByTestId('workdir-browse')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('workdir-recent')).not.toBeInTheDocument();
  });
});
