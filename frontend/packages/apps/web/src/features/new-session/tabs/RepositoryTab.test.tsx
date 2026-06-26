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
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../../data/apiContext';
import { useComposerStore } from '../../../store/composerStore';
import { RepositoryTab } from './RepositoryTab';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderTab() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <RepositoryTab />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('RepositoryTab', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionWorkdir: null,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: { kind: 'head' },
      newSessionSelectedPrUrl: null,
    });
  });

  it('mount alone does NOT write to the composer store', async () => {
    // The initial highlight of the first repo is local UI state only.
    // If the tab wrote into newSessionWorkdir on mount, that workdir
    // would leak into the PR / Directory tabs the user later switches
    // to. Just opening the New session screen must never produce a
    // workdir.
    renderTab();
    await waitFor(() => {
      expect(
        screen.getAllByTestId('repository-tab-repo-row').length,
      ).toBeGreaterThan(0);
    });
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
  });

  it('clicking a repo row auto-picks its recently_used_clone_path', async () => {
    // The first mock repo (x7c1/delta) has recently_used_clone_path =
    // /home/dev/projects/delta. Clicking its row both highlights it
    // and writes the default clone into newSessionWorkdir, so the user
    // can press Send without first clicking a clone row.
    renderTab();
    const repoRows = await screen.findAllByTestId('repository-tab-repo-row');
    const deltaRow = repoRows.find((row) =>
      row.textContent?.includes('x7c1/delta'),
    );
    expect(deltaRow).toBeDefined();
    fireEvent.click(deltaRow!);

    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      );
    });
  });

  it('switching to a different repo replaces the picked clone with that repo default', async () => {
    renderTab();
    const repoRows = await screen.findAllByTestId('repository-tab-repo-row');
    const deltaRow = repoRows.find((row) =>
      row.textContent?.includes('x7c1/delta'),
    );
    expect(deltaRow).toBeDefined();
    fireEvent.click(deltaRow!);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      );
    });

    // The second mock repo (`website`) has a single clone at
    // /home/dev/projects/website. Clicking it must replace the previous
    // pick — the previous selection does not belong to this repo.
    const websiteRow = repoRows.find((row) =>
      row.textContent?.includes('website'),
    );
    expect(websiteRow).toBeDefined();
    fireEvent.click(websiteRow!);

    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/website',
      );
    });
  });

  it('reclicking the same repo does NOT stomp an explicit clone pick from that repo', async () => {
    renderTab();
    // First, land on the x7c1/delta repo (two clones) and let its
    // default clone be auto-picked.
    const repoRows = await screen.findAllByTestId('repository-tab-repo-row');
    const deltaRow = repoRows.find((row) =>
      row.textContent?.includes('x7c1/delta'),
    );
    expect(deltaRow).toBeDefined();
    fireEvent.click(deltaRow!);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      );
    });

    // Then pick the non-default clone of the same repo explicitly.
    const cloneRows = await screen.findAllByTestId('repository-tab-clone-row');
    const forkRow = cloneRows.find((row) =>
      row.textContent?.includes('delta-fork'),
    );
    expect(forkRow).toBeDefined();
    fireEvent.click(forkRow!);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta-fork',
      );
    });

    // Click the same x7c1/delta repo row again. The handler must see
    // that the current selection already belongs to this repo and leave
    // it alone — not snap back to recently_used_clone_path.
    fireEvent.click(deltaRow!);
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta-fork',
    );
  });
});
