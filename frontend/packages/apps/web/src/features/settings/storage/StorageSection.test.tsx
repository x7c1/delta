import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, MOCK_DATA_DIR, mockStorage } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../../data/apiContext';
import { ErasedGate } from './ErasedGate';
import { StorageSection } from './StorageSection';
import { formatBytes } from './formatBytes';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

function renderSection(active = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <ErasedGate>
          <StorageSection active={active} />
        </ErasedGate>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('StorageSection', () => {
  it('lists every reported path, plus the identifier and version', async () => {
    renderSection();

    const list = await screen.findByTestId('storage-list');
    for (const path of [
      MOCK_DATA_DIR,
      mockStorage.database.path,
      mockStorage.hook_state,
      mockStorage.sessions_dir,
      mockStorage.session_settings,
      mockStorage.tmux_conf,
      mockStorage.tmux_socket,
      mockStorage.worktree_base,
      mockStorage.transcript_root,
    ]) {
      expect(within(list).getAllByTitle(path, { exact: true }).length).toBeGreaterThan(0);
    }
    const identity = screen.getByTestId('storage-identity');
    expect(identity).toHaveTextContent(mockStorage.identifier);
    expect(identity).toHaveTextContent(mockStorage.version);
  });

  it('shows the database size humanised with the exact count in a tooltip', async () => {
    renderSection();

    const database = await screen.findByTestId('storage-database');
    const size = within(database).getByText('3.1 MB');
    expect(size).toHaveAttribute('title', '3,250,176 bytes');
    const snapshots = within(database).getByTestId('storage-snapshots');
    expect(
      within(snapshots).getByTitle(mockStorage.snapshots[0].path, { exact: true }),
    ).toBeInTheDocument();
    expect(within(snapshots).getByText('1.0 MB')).toBeInTheDocument();
  });

  it('says so in one line when there are no snapshots', async () => {
    server.use(
      http.get('*/api/storage', () =>
        HttpResponse.json({ ...mockStorage, snapshots: [] }),
      ),
    );
    renderSection();

    const database = await screen.findByTestId('storage-database');
    expect(within(database).getByTestId('storage-no-snapshots')).toHaveTextContent(
      'No migration snapshots.',
    );
    expect(within(database).queryByTestId('storage-snapshots')).toBeNull();
  });

  it('offers a retry when the inventory cannot be loaded', async () => {
    server.use(
      http.get('*/api/storage', () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 }),
      ),
    );
    renderSection();

    expect(await screen.findByText('Could not load storage.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });

  describe('copy control', () => {
    afterEach(() => vi.unstubAllGlobals());

    it('confirms the copy on the button', async () => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
      renderSection();

      const copy = await screen.findByRole('button', { name: 'Copy Data directory' });
      fireEvent.click(copy);

      expect(await within(copy).findByText('Copied')).toBeInTheDocument();
      expect(writeText).toHaveBeenCalledWith(MOCK_DATA_DIR);
    });

    it('says the copy failed when there is no clipboard to write to', async () => {
      vi.stubGlobal('navigator', { ...navigator, clipboard: undefined });
      renderSection();

      const copy = await screen.findByRole('button', { name: 'Copy Data directory' });
      fireEvent.click(copy);

      expect(await within(copy).findByText('Copy failed')).toBeInTheDocument();
    });
  });

  it('does not fetch while inactive', () => {
    renderSection(false);
    expect(screen.getByText('Storage')).toBeInTheDocument();
    expect(screen.queryByTestId('storage-list')).toBeNull();
  });
});

describe('formatBytes', () => {
  it('humanises in powers of 1024 with one decimal above a kilobyte', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1024 ** 2 - 1)).toBe('1.0 MB');
    expect(formatBytes(3_250_176)).toBe('3.1 MB');
    expect(formatBytes(5 * 1024 ** 3)).toBe('5.0 GB');
  });
});
