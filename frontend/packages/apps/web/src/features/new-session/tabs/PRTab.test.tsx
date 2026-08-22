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
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../../data/apiContext';
import {
  DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
  useComposerStore,
} from '../../../store/composerStore';
import { useLiveStore } from '../../../store/liveStore';
import { useSettingsStore } from '../../../store/settingsStore';
import { Composer } from '../../composer/Composer';
import { ProviderTabs } from '../../composer/ProviderTabs';
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
      newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
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
      // The pick records its provenance, which is what locks the worktree UI
      // to the PR's head branch and highlights the row.
      expect(state.newSessionWorkdirSource).toEqual({
        kind: 'pr',
        url: 'https://github.com/x7c1/delta/pull/174',
        number: 174,
        repo_owner: 'x7c1',
        repo_name: 'delta',
        head_ref: 'feat/repo-tab',
      });
    });
  });

  it('clicking a row whose repo has no local clone opens the inline clone panel without touching the composer', async () => {
    renderTab();
    // The "x7c1/other" fixture has `has_local_clone: false`. The row is still
    // de-emphasised (it cannot start a session yet) but is now the way out:
    // clicking it expands the clone panel rather than doing nothing.
    await screen.findByTestId('pr-tab-reviewer');
    const rows = await screen.findAllByTestId('pr-tab-row');
    const noClone = rows.find(
      (row) => row.getAttribute('data-has-local-clone') === 'false',
    );
    expect(noClone).toBeDefined();
    expect(noClone).toHaveAttribute('aria-expanded', 'false');
    expect(noClone?.querySelector('[data-testid="pr-tab-row-no-clone-hint"]'))
      .not.toBeNull();
    expect(screen.queryByTestId('pr-tab-clone-panel')).toBeNull();

    fireEvent.click(noClone!);

    expect(await screen.findByTestId('pr-tab-clone-panel')).toBeInTheDocument();
    expect(noClone).toHaveAttribute('aria-expanded', 'true');
    // Opening the panel is not a pick: the composer is untouched until a clone
    // actually lands.
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
      expect(row.className).not.toMatch(/bg-accent\/10/);
      expect(row.className).not.toMatch(/ring-accent-disabled/);
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
      expect(useComposerStore.getState().newSessionWorkdirSource).toMatchObject({
        kind: 'pr',
        url: 'https://github.com/x7c1/delta/pull/174',
      });
    });

    rows = await screen.findAllByTestId('pr-tab-row');
    const picked = rows.find(
      (row) => row.getAttribute('data-has-local-clone') === 'true',
    )!;
    const others = rows.filter((row) => row !== picked);
    expect(picked).toHaveAttribute('data-selected', 'true');
    expect(picked.className).toMatch(/bg-accent\/10/);
    expect(picked.className).toMatch(/ring-accent-disabled/);
    for (const row of others) {
      expect(row).toHaveAttribute('data-selected', 'false');
    }

    // A disabled click is a no-op: the store retains the prior pick.
    fireEvent.click(disabledRow!);
    expect(useComposerStore.getState().newSessionWorkdirSource).toMatchObject({
      kind: 'pr',
      url: 'https://github.com/x7c1/delta/pull/174',
    });
  });

  it('drops the highlight when a directory pick replaces the PR provenance', async () => {
    // The highlight reads the workdir's provenance, so a Repository / Directory
    // pick (which stamps `directory`) takes it with it — no row is left
    // claiming to be the active pick while the session starts somewhere else.
    renderTab();
    await screen.findByTestId('pr-tab-reviewer');
    const clickable = (await screen.findAllByTestId('pr-tab-row')).find(
      (row) => row.getAttribute('data-has-local-clone') === 'true',
    );
    fireEvent.click(clickable!);
    await waitFor(() => {
      expect(
        screen.getByTitle('https://github.com/x7c1/delta/pull/174'),
      ).toHaveAttribute('data-selected', 'true');
    });

    act(() => {
      useComposerStore
        .getState()
        .setNewSessionWorkdir('/home/dev/projects/other');
    });

    for (const row of await screen.findAllByTestId('pr-tab-row')) {
      expect(row).toHaveAttribute('data-selected', 'false');
    }
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
    // string — the prior `font-semibold text-fg` first-row bump
    // would surface as a class divergence here.
    const labelClasses = rows.map((row) => {
      const label = row.querySelector('span.font-mono');
      return label?.getAttribute('class') ?? '';
    });
    expect(new Set(labelClasses).size).toBe(1);
    // And the surviving class must NOT contain `font-semibold` — the
    // single uniform style is the same `text-fg` weight the
    // Repository tab's clickable rows use, so the two tabs read
    // consistently. The divider supplies the grouping structure.
    expect(labelClasses[0]).not.toMatch(/font-semibold/);
    expect(labelClasses[0]).toMatch(/text-fg/);
  });
});

describe('PRTab → new-session send (provider threading)', () => {
  beforeEach(() => {
    // A fresh compose: no pick, Claude default provider, and unseeded so the
    // selector seeds from the persisted default set below.
    useComposerStore.setState({
      newSessionWorkdir: null,
      newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: { kind: 'head' },
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
    });
    useSettingsStore.setState({ defaultProvider: 'claude' });
  });

  /**
   * Render the pieces TranscriptPane composes for a new session started from
   * the PR tab — the provider selector, the PR list, and the composer — over
   * the shared composer store, and capture the body of the `POST /api/sends`
   * the composer fires. This exercises the real PR-origin path: a PR click
   * pre-fills the worktree/workdir, the selector sets the provider, and the
   * composer assembles the send from that store state.
   */
  function renderPrFlowAndCaptureSend(): { read: () => unknown } {
    let captured: unknown;
    server.use(
      http.post('*/api/sends', async ({ request }) => {
        captured = await request.json();
        return HttpResponse.json(
          {
            send: {
              id: 0,
              session_id: '',
              thread_id: 0,
              semantic_parent_uuid: null,
              text: 'irrelevant',
              locator_quote: null,
              status: 'dispatched',
              matched_uuid: null,
              created_at: '2026-01-01T00:00:00Z',
            },
          },
          { status: 201 },
        );
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const client = new ApiClient({ baseUrl: 'http://localhost' });
    render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={client}>
          <ProviderTabs />
          <PRTab />
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );
    return { read: () => captured };
  }

  it('starting from a selected PR with Codex chosen sends provider "codex" with the PR worktree', async () => {
    const { read } = renderPrFlowAndCaptureSend();

    // Pick the PR whose repo has a local clone: this pre-fills the workdir and
    // a `use_remote_branch` worktree keyed to the PR's head branch.
    fireEvent.click(
      await screen.findByTitle('https://github.com/x7c1/delta/pull/174'),
    );
    await waitFor(() =>
      expect(useComposerStore.getState().newSessionWorktreeEnabled).toBe(true),
    );

    // Choose Codex in the provider selector (the top-level axis of the flow).
    fireEvent.click(
      within(screen.getByTestId('provider-option-codex')).getByRole('radio'),
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume this PR' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'resume this PR',
        workdir: '/home/dev/projects/delta',
        worktree: {
          start_point: { kind: 'use_remote_branch', name: 'feat/repo-tab' },
        },
        provider: 'codex',
      });
    });
  });

  it('starting from a selected PR on the Claude default omits provider but keeps the PR worktree', async () => {
    // Claude stays byte-identical to today's PR-origin send: the worktree is
    // present, but `provider` is omitted (the backend defaults it to Claude).
    const { read } = renderPrFlowAndCaptureSend();

    fireEvent.click(
      await screen.findByTitle('https://github.com/x7c1/delta/pull/174'),
    );
    await waitFor(() =>
      expect(useComposerStore.getState().newSessionWorktreeEnabled).toBe(true),
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume this PR' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'resume this PR',
        workdir: '/home/dev/projects/delta',
        worktree: {
          start_point: { kind: 'use_remote_branch', name: 'feat/repo-tab' },
        },
      });
    });
  });
});

describe('PRTab → inline clone panel', () => {
  /** The no-clone reviewer fixture the mock backend seeds. */
  const NO_CLONE_PR_URL = 'https://github.com/x7c1/other/pull/9';

  beforeEach(() => {
    useComposerStore.setState({
      newSessionWorkdir: null,
      newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: { kind: 'head' },
    });
    // The clone state is live-channel state, so it survives across renders the
    // way the store does; reset it so each test starts with no intent.
    useLiveStore.setState({
      cloneIntent: null,
      cloneCompletion: null,
      cloneFailure: null,
    });
  });

  /** Open the no-clone row's clone panel and return it. */
  async function openClonePanel(): Promise<HTMLElement> {
    fireEvent.click(await screen.findByTitle(/No local clone/));
    return screen.findByTestId('pr-tab-clone-panel');
  }

  it('offers a registration input when no clone root is registered, and registers before cloning', async () => {
    // The mock backend seeds zero clone roots, which is the cold-start case:
    // the user has never told Delta where clones live. Sending them to Settings
    // mid-decision is the thing this panel exists to avoid, so registration is
    // a step of the flow.
    const requests: unknown[] = [];
    server.use(
      http.post('*/api/clone-roots', async ({ request }) => {
        requests.push({ route: 'clone-roots', body: await request.json() });
        return HttpResponse.json({ path: '/home/dev/code' }, { status: 201 });
      }),
      http.post('*/api/repositories/clone', async ({ request }) => {
        requests.push({ route: 'clone', body: await request.json() });
        return new HttpResponse(null, { status: 202 });
      }),
    );
    renderTab();
    const panel = await openClonePanel();

    const input = within(panel).getByTestId('pr-tab-clone-root-input');
    // Typed with a trailing slash, the way a shell completion hands it over.
    // Registration canonicalises that away, and the clone has to follow the
    // canonical spelling: the server matches a registered root verbatim, so
    // cloning into the typed form would be refused as unregistered moments
    // after registering it.
    fireEvent.change(input, { target: { value: '/home/dev/code/' } });
    // The destination is shown before committing, so "where will this land?" is
    // answered without having to guess the naming rule.
    expect(within(panel).getByTestId('pr-tab-clone-destination')).toHaveTextContent(
      '/home/dev/code/other',
    );

    fireEvent.click(within(panel).getByTestId('pr-tab-clone-start'));

    await waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[0]).toEqual({
      route: 'clone-roots',
      body: { path: '/home/dev/code/' },
    });
    expect(requests[1]).toEqual({
      route: 'clone',
      body: {
        repo_owner: 'x7c1',
        repo_name: 'other',
        clone_root: '/home/dev/code',
      },
    });
    // The intent is recorded only once the server accepted, and it names the
    // destination the completion event will carry.
    await waitFor(() =>
      expect(useLiveStore.getState().cloneIntent).toMatchObject({
        destination: '/home/dev/code/other',
      }),
    );
  });

  it('offers a selector defaulting to the most recently registered root when several exist', async () => {
    // The list arrives newest-first, and the newest root is the one the user
    // most likely means — but it stays a choice, because a machine with several
    // clone roots has them for a reason.
    let cloned: unknown;
    server.use(
      http.get('*/api/clone-roots', () =>
        HttpResponse.json({
          clone_roots: [{ path: '/home/dev/newest' }, { path: '/home/dev/older' }],
        }),
      ),
      http.post('*/api/repositories/clone', async ({ request }) => {
        cloned = await request.json();
        return new HttpResponse(null, { status: 202 });
      }),
    );
    renderTab();
    const panel = await openClonePanel();

    const select = within(panel).getByTestId('pr-tab-clone-root-select');
    expect(select).toHaveValue('/home/dev/newest');
    expect(within(panel).getByTestId('pr-tab-clone-destination')).toHaveTextContent(
      '/home/dev/newest/other',
    );

    fireEvent.change(select, { target: { value: '/home/dev/older' } });
    fireEvent.click(within(panel).getByTestId('pr-tab-clone-start'));

    await waitFor(() =>
      expect(cloned).toEqual({
        repo_owner: 'x7c1',
        repo_name: 'other',
        clone_root: '/home/dev/older',
      }),
    );
  });

  it('auto-continues into the PR pick when the completion event lands while the intent is active', async () => {
    server.use(
      http.get('*/api/clone-roots', () =>
        HttpResponse.json({ clone_roots: [{ path: '/home/dev/code' }] }),
      ),
      http.post('*/api/repositories/clone', () =>
        new HttpResponse(null, { status: 202 }),
      ),
    );
    renderTab();
    const panel = await openClonePanel();
    // Exactly one registered root: no choice to make, so the panel just names
    // the destination.
    expect(within(panel).getByTestId('pr-tab-clone-root-single')).toHaveTextContent(
      '/home/dev/code',
    );

    fireEvent.click(within(panel).getByTestId('pr-tab-clone-start'));
    await waitFor(() =>
      expect(useLiveStore.getState().cloneIntent).not.toBeNull(),
    );

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'repository_clone_completed',
        repo_owner: 'x7c1',
        repo_name: 'other',
        clone_root: '/home/dev/code',
        destination_path: '/home/dev/code/other',
      });
    });

    // The whole point: the user asked for this PR, so the clone landing walks
    // them straight into the pick — workdir plus the locked PR provenance —
    // with the freshly cloned path as the working directory.
    await waitFor(() => {
      const state = useComposerStore.getState();
      expect(state.newSessionWorkdir).toBe('/home/dev/code/other');
      expect(state.newSessionWorktreeEnabled).toBe(true);
      expect(state.newSessionWorkdirSource).toMatchObject({
        kind: 'pr',
        url: NO_CLONE_PR_URL,
      });
    });
    // The one-shot signal is consumed, so re-mounting later cannot replay it.
    expect(useLiveStore.getState().cloneCompletion).toBeNull();
  });

  it('leaves the composer alone when the completion event lands after the intent was superseded', async () => {
    server.use(
      http.get('*/api/clone-roots', () =>
        HttpResponse.json({ clone_roots: [{ path: '/home/dev/code' }] }),
      ),
      http.post('*/api/repositories/clone', () =>
        new HttpResponse(null, { status: 202 }),
      ),
    );
    renderTab();
    const panel = await openClonePanel();
    fireEvent.click(within(panel).getByTestId('pr-tab-clone-start'));
    await waitFor(() =>
      expect(useLiveStore.getState().cloneIntent).not.toBeNull(),
    );

    // The user moves on: picks the PR that already has a clone. That pick is
    // what the composer must keep — a clone finishing afterwards has no claim
    // on it.
    fireEvent.click(
      await screen.findByTitle('https://github.com/x7c1/delta/pull/174'),
    );
    await waitFor(() =>
      expect(useComposerStore.getState().newSessionWorkdir).toBe(
        '/home/dev/projects/delta',
      ),
    );
    expect(useLiveStore.getState().cloneIntent).toBeNull();

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'repository_clone_completed',
        repo_owner: 'x7c1',
        repo_name: 'other',
        clone_root: '/home/dev/code',
        destination_path: '/home/dev/code/other',
      });
    });

    // Nothing moved: the superseded clone neither re-points the workdir nor
    // steals the row highlight.
    expect(useLiveStore.getState().cloneCompletion).toBeNull();
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
    expect(useComposerStore.getState().newSessionWorkdirSource).toMatchObject({
      kind: 'pr',
      url: 'https://github.com/x7c1/delta/pull/174',
    });
  });

  it('shows the refusal inline when the destination already exists', async () => {
    server.use(
      http.get('*/api/clone-roots', () =>
        HttpResponse.json({ clone_roots: [{ path: '/home/dev/projects' }] }),
      ),
      http.post('*/api/repositories/clone', () =>
        HttpResponse.json(
          {
            error: 'clone destination already exists: /home/dev/projects/other',
            code: 'clone_dest_exists',
          },
          { status: 409 },
        ),
      ),
    );
    renderTab();
    const panel = await openClonePanel();
    fireEvent.click(within(panel).getByTestId('pr-tab-clone-start'));

    expect(await screen.findByTestId('pr-tab-clone-error')).toHaveTextContent(
      'already exists',
    );
    // A refused request starts nothing, so no row is left spinning.
    expect(useLiveStore.getState().cloneIntent).toBeNull();
  });

  it('surfaces a failed clone job inline, leaving the row retryable', async () => {
    server.use(
      http.get('*/api/clone-roots', () =>
        HttpResponse.json({ clone_roots: [{ path: '/home/dev/code' }] }),
      ),
      http.post('*/api/repositories/clone', () =>
        new HttpResponse(null, { status: 202 }),
      ),
    );
    renderTab();
    const panel = await openClonePanel();
    fireEvent.click(within(panel).getByTestId('pr-tab-clone-start'));
    await waitFor(() =>
      expect(useLiveStore.getState().cloneIntent).not.toBeNull(),
    );

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'repository_clone_failed',
        repo_owner: 'x7c1',
        repo_name: 'other',
        clone_root: '/home/dev/code',
        destination_path: '/home/dev/code/other',
        message: 'could not resolve host github.com',
      });
    });

    expect(await screen.findByTestId('pr-tab-clone-error')).toHaveTextContent(
      'could not resolve host github.com',
    );
    // The intent is retired, so the Clone button is live again — retrying is
    // simply clicking it.
    expect(useLiveStore.getState().cloneIntent).toBeNull();
    expect(screen.getByTestId('pr-tab-clone-start')).not.toBeDisabled();
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
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
