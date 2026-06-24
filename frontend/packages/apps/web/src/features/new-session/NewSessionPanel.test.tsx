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
import {
  DEFAULT_NEW_SESSION_TAB,
  useComposerStore,
} from '../../store/composerStore';
import { NewSessionPanel } from './NewSessionPanel';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <NewSessionPanel />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('NewSessionPanel tab container', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionTab: DEFAULT_NEW_SESSION_TAB,
      newSessionWorkdir: null,
    });
  });

  it('lands on the Repository tab by default', async () => {
    renderPanel();
    expect(screen.getByTestId('new-session-tab-repository')).toHaveAttribute(
      'aria-selected',
      'true',
    );
    // The list arrives once the mocked /api/repositories query resolves.
    expect(await screen.findByTestId('repository-tab')).toBeInTheDocument();
  });

  it('switches tabs on click and persists the choice to the store', () => {
    renderPanel();
    fireEvent.click(screen.getByTestId('new-session-tab-pr'));
    expect(useComposerStore.getState().newSessionTab).toBe('pr');
    expect(screen.getByTestId('new-session-pr-tab')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('new-session-tab-directory'));
    expect(useComposerStore.getState().newSessionTab).toBe('directory');
    expect(screen.getByTestId('new-session-directory-tab')).toBeInTheDocument();
  });
});

describe('RepositoryTab', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionTab: 'repository',
      newSessionWorkdir: null,
    });
  });

  it('lists registered repositories from /api/repositories', async () => {
    renderPanel();
    // The mock seeds two repos; both render with their display_name.
    expect(await screen.findByText('x7c1/delta')).toBeInTheDocument();
    expect(screen.getByText('website')).toBeInTheDocument();
  });

  it('selecting a clone writes it into newSessionWorkdir', async () => {
    renderPanel();
    // The first (most-recent) repo, x7c1/delta, is pre-selected, so its
    // clones render straight away.
    const cloneRows = await screen.findAllByTestId(
      'repository-tab-clone-row',
    );
    // The recently-used clone is the first row (`/home/dev/projects/delta`).
    // Click the SECOND clone (the non-default) to verify the click
    // mechanism writes the picked path into the store.
    fireEvent.click(cloneRows[1]);
    await waitFor(() =>
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta-fork',
      ),
    );
  });

  it('shows an empty-state hint when the repo list is empty', async () => {
    // Override the default mock with an empty list.
    server.use(
      http.get('*/api/repositories', () =>
        HttpResponse.json({ repositories: [] }),
      ),
    );
    renderPanel();
    expect(
      await screen.findByTestId('repository-tab-empty'),
    ).toHaveTextContent('No repositories yet');
  });
});

describe('DirectoryTab', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionTab: 'directory',
      newSessionWorkdir: null,
    });
  });

  it('renders the Recent + Browse picker inline (no modal)', async () => {
    renderPanel();
    // The picker's Recent header from the mocked /api/workdir/recent
    // surfaces inline rather than inside a dialog.
    expect(
      await screen.findByTestId('workdir-recent'),
    ).toBeInTheDocument();
    expect(screen.getByTestId('workdir-browse')).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('clicking a Recent row commits it as the workdir straight away', async () => {
    renderPanel();
    const row = await screen.findByTitle('/home/dev/projects/delta');
    fireEvent.click(row);
    await waitFor(() =>
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      ),
    );
  });
});

describe('RepositoryTab spawn flow', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionTab: 'repository',
      newSessionWorkdir: null,
    });
  });

  it('clicking a non-default clone pre-fills newSessionWorkdir for a subsequent send', async () => {
    renderPanel();

    // Wait for the rendered clone rows from the seeded mock (two clones
    // under the bundled x7c1/delta repo).
    const cloneRows = await screen.findAllByTestId(
      'repository-tab-clone-row',
    );
    expect(cloneRows.length).toBeGreaterThanOrEqual(2);

    // The first row is the default (`recently_used_clone_path`); flag it.
    expect(cloneRows[0]).toHaveAttribute('data-default', 'true');
    expect(cloneRows[1]).toHaveAttribute('data-default', 'false');

    // Click the non-default clone — this is the gesture the user makes to
    // pick a different clone of the same repo.
    fireEvent.click(cloneRows[1]);

    // The composer store now holds the picked dir, ready for the Composer
    // to send as `workdir`. The send body assembly itself is exercised by
    // Composer.test.tsx; pinning the store contract here is enough.
    await waitFor(() =>
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta-fork',
      ),
    );

    // And the picked row is highlighted (aria-pressed).
    expect(cloneRows[1]).toHaveAttribute('aria-pressed', 'true');
  });
});
