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
import { ApiProvider } from '../../../data/apiContext';
import { useComposerStore } from '../../../store/composerStore';
import { PRTab } from './PRTab';

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
        <PRTab />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('PRTab', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionWorkdir: null,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: { kind: 'head' },
    });
  });

  it('renders both lenses with the seeded mock data', async () => {
    renderTab();
    // The reviewer lens seeds two PRs (one with a local clone, one
    // without); the author lens seeds one (a draft on x7c1/delta).
    expect(await screen.findByTestId('pr-tab-reviewer')).toBeInTheDocument();
    expect(screen.getByTestId('pr-tab-author')).toBeInTheDocument();
    expect(
      screen.getByText('feat: add Repository tab to the new-session screen'),
    ).toBeInTheDocument();
    expect(screen.getByText('wip: my own draft')).toBeInTheDocument();
  });

  it('clicking a row whose repo has a local clone pre-fills the composer', async () => {
    renderTab();
    // Wait for the row whose title matches the clone-having PR fixture.
    const cloneRow = await screen.findByTitle(
      'https://github.com/x7c1/delta/pull/174',
    );
    fireEvent.click(cloneRow);

    await waitFor(() => {
      const state = useComposerStore.getState();
      expect(state.newSessionWorkdir).toBe('/home/dev/projects/delta');
      expect(state.newSessionWorktreeEnabled).toBe(true);
      expect(state.newSessionWorktreeStartPoint).toEqual({
        kind: 'use_remote_branch',
        name: 'feat/repo-tab',
      });
    });
  });

  it('clicking a row whose repo has no local clone is a no-op + shows the inline hint', async () => {
    renderTab();
    // The "x7c1/other" fixture has `has_local_clone: false`. The row
    // is rendered (so the inline hint is reachable) but `aria-disabled`
    // and visually de-emphasised.
    await screen.findByTestId('pr-tab-reviewer');
    const rows = await screen.findAllByTestId('pr-tab-row');
    const noClone = rows.find(
      (row) => row.getAttribute('data-has-local-clone') === 'false',
    );
    expect(noClone).toBeDefined();
    expect(noClone).toHaveAttribute('aria-disabled', 'true');
    // The inline help row text mentions the unblock command.
    expect(noClone?.querySelector('[data-testid="pr-tab-row-no-clone-hint"]'))
      .not.toBeNull();

    fireEvent.click(noClone!);
    // No state change happened.
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
    expect(useComposerStore.getState().newSessionWorktreeEnabled).toBe(false);
  });

  it('shows the "run gh auth login" hint when gh is unavailable', async () => {
    // Override the default mock with the unauthenticated branch:
    // gh_available=false, empty list. The PR tab still mounts and
    // renders the inline hint rather than a generic error.
    server.use(
      http.get('*/api/prs', () =>
        HttpResponse.json({ gh_available: false, pull_requests: [] }),
      ),
    );
    renderTab();
    expect(
      await screen.findByTestId('pr-tab-gh-unavailable'),
    ).toHaveTextContent('gh auth login');
  });
});
