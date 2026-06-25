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
      newSessionSelectedPrUrl: null,
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

  it('renders a divider above the first row of each repo group, but not above the very first row in a section', async () => {
    // Three reviewer PRs across two repos (two in `acme/widgets`,
    // one in `acme/zeta`) so a section contains both an intra-repo
    // boundary (no divider) and an inter-repo boundary (divider).
    // The author lens stays at one PR so the second section can
    // also verify "first row of a section gets no divider above".
    server.use(
      http.get('*/api/prs', ({ request }) => {
        const lens = new URL(request.url).searchParams.get('lens');
        if (lens === 'reviewer') {
          return HttpResponse.json({
            gh_available: true,
            pull_requests: [
              prFixture({
                number: 1,
                repo_owner: 'acme',
                repo_name: 'widgets',
                updated_at: '2026-06-24T10:00:00Z',
              }),
              prFixture({
                number: 2,
                repo_owner: 'acme',
                repo_name: 'widgets',
                updated_at: '2026-06-23T10:00:00Z',
              }),
              prFixture({
                number: 3,
                repo_owner: 'acme',
                repo_name: 'zeta',
                updated_at: '2026-06-22T10:00:00Z',
              }),
            ],
          });
        }
        return HttpResponse.json({
          gh_available: true,
          pull_requests: [
            prFixture({
              number: 100,
              repo_owner: 'acme',
              repo_name: 'widgets',
              updated_at: '2026-06-21T10:00:00Z',
            }),
          ],
        });
      }),
    );
    renderTab();

    const reviewerSection = await screen.findByTestId('pr-tab-reviewer');
    const authorSection = await screen.findByTestId('pr-tab-author');

    // Reviewer section: 3 rows, exactly 1 divider — between the two
    // `acme/widgets` rows and the lone `acme/zeta` row. No divider
    // sits above the very first row.
    const reviewerDividers = reviewerSection.querySelectorAll(
      '[data-testid="pr-tab-repo-divider"]',
    );
    expect(reviewerDividers).toHaveLength(1);
    // Author section: 1 row, no divider above it.
    const authorDividers = authorSection.querySelectorAll(
      '[data-testid="pr-tab-repo-divider"]',
    );
    expect(authorDividers).toHaveLength(0);

    // The first item of the reviewer `<ul>` is a row, not a divider.
    const reviewerList = reviewerSection.querySelector('ul');
    expect(reviewerList).not.toBeNull();
    expect(reviewerList!.firstElementChild?.getAttribute('data-testid')).not.toBe(
      'pr-tab-repo-divider',
    );
  });

  it('highlights only the clicked row, and a disabled click leaves the prior selection alone', async () => {
    // Initial state: no row carries the selected styling — covers the
    // "fresh tab open, no pick yet" baseline. Then click a clickable PR
    // and assert that the indigo highlight lands on exactly that row.
    // Finally click a disabled (no-local-clone) row and assert the
    // previous selection is untouched, because the disabled click is a
    // silent no-op.
    renderTab();
    await screen.findByTestId('pr-tab-reviewer');
    let rows = await screen.findAllByTestId('pr-tab-row');
    for (const row of rows) {
      expect(row).toHaveAttribute('data-selected', 'false');
      expect(row.className).not.toMatch(/bg-indigo-50/);
      expect(row.className).not.toMatch(/ring-indigo-200/);
    }

    const clickable = rows.find(
      (row) => row.getAttribute('data-has-local-clone') === 'true',
    );
    const disabledRow = rows.find(
      (row) => row.getAttribute('data-has-local-clone') === 'false',
    );
    expect(clickable).toBeDefined();
    expect(disabledRow).toBeDefined();

    fireEvent.click(clickable!);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionSelectedPrUrl).toBe(
        'https://github.com/x7c1/delta/pull/174',
      );
    });

    rows = await screen.findAllByTestId('pr-tab-row');
    const picked = rows.find(
      (row) => row.getAttribute('data-has-local-clone') === 'true',
    )!;
    const others = rows.filter((row) => row !== picked);
    expect(picked).toHaveAttribute('data-selected', 'true');
    expect(picked.className).toMatch(/bg-indigo-50/);
    expect(picked.className).toMatch(/ring-indigo-200/);
    for (const row of others) {
      expect(row).toHaveAttribute('data-selected', 'false');
    }

    // A disabled click is a no-op: the store retains the prior url.
    fireEvent.click(disabledRow!);
    expect(useComposerStore.getState().newSessionSelectedPrUrl).toBe(
      'https://github.com/x7c1/delta/pull/174',
    );
  });

  it('renders every repo label with the same class — no font-weight conditional left over', async () => {
    // The reviewer lens default fixture is two PRs across two repos
    // (`x7c1/delta`, `x7c1/other`), which exercises both the first-of-
    // group and the second-of-group code paths from the prior design.
    // After the divider rework both labels must share one class.
    renderTab();
    await screen.findByTestId('pr-tab-reviewer');
    const rows = await screen.findAllByTestId('pr-tab-row');
    expect(rows.length).toBeGreaterThanOrEqual(2);
    // The repo label is the row's first inner `<span>` carrying the
    // `font-mono` class. All such labels must share an identical class
    // string — the prior `font-semibold text-slate-700` first-row bump
    // would surface as a class divergence here.
    const labelClasses = rows.map((row) => {
      const label = row.querySelector('span.font-mono');
      return label?.getAttribute('class') ?? '';
    });
    expect(new Set(labelClasses).size).toBe(1);
    // And the surviving class must NOT contain `font-semibold` — the
    // single uniform style is the same `text-slate-700` weight the
    // Repository tab's clickable rows use, so the two tabs read
    // consistently. The divider supplies the grouping structure.
    expect(labelClasses[0]).not.toMatch(/font-semibold/);
    expect(labelClasses[0]).toMatch(/text-slate-700/);
  });
});

// Minimal PR factory for fixtures that exercise the repo-grouping
// divider rendering. Only fills the fields PRTab actually reads.
function prFixture(overrides: {
  number: number;
  repo_owner: string;
  repo_name: string;
  updated_at: string;
}) {
  return {
    number: overrides.number,
    title: `pr #${overrides.number}`,
    repo_owner: overrides.repo_owner,
    repo_name: overrides.repo_name,
    head_ref: `feat/${overrides.number}`,
    head_repo_owner: overrides.repo_owner,
    head_repo_name: overrides.repo_name,
    draft: false,
    url: `https://github.com/${overrides.repo_owner}/${overrides.repo_name}/pull/${overrides.number}`,
    updated_at: overrides.updated_at,
    author_login: 'someone',
    has_local_clone: false,
  };
}
