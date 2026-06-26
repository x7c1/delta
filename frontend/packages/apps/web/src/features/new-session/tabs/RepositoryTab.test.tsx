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

  it("auto-picks the first repo's recently_used_clone_path on initial render", async () => {
    // The first mock repo (x7c1/delta) has recently_used_clone_path
    // = /home/dev/projects/delta. With no prior selection, the tab should
    // auto-select that repo AND auto-pick its default clone on mount, so
    // both the repo row and a clone row read as active and the user can
    // press Send without first clicking a clone row.
    renderTab();
    await screen.findByTestId('repository-tab-repos');
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      );
    });
  });

  it('switching to a different repo replaces the picked clone with that repo default', async () => {
    renderTab();
    await screen.findByTestId('repository-tab-repos');
    // After the initial auto-pick lands, pick a different repo.
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      );
    });

    // The second mock repo (`website`) has a single clone at
    // /home/dev/projects/website. Clicking it must replace the previous
    // pick — the previous selection does not belong to this repo.
    const repoRows = await screen.findAllByTestId('repository-tab-repo-row');
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
    await screen.findByTestId('repository-tab-repos');
    // Wait for the initial auto-pick on the x7c1/delta repo (two clones).
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
    const repoRows = await screen.findAllByTestId('repository-tab-repo-row');
    const deltaRow = repoRows.find((row) =>
      row.textContent?.includes('x7c1/delta'),
    );
    expect(deltaRow).toBeDefined();
    fireEvent.click(deltaRow!);
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta-fork',
    );
  });
});
