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
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, MOCK_GIT_REPO_ROOT } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import {
  DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
  DEFAULT_WORKTREE_START_POINT,
  useComposerStore,
} from '../../store/composerStore';
import { WorktreeOptions } from './WorktreeOptions';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** A directory outside the mock git repository. */
const NON_GIT_DIR = '/home/dev/scratch';

function renderOptions() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <WorktreeOptions />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('WorktreeOptions', () => {
  beforeEach(() => {
    useComposerStore.setState({
      newSessionWorkdir: null,
      // Every test in this block is a directory pick — the PR-provenance lock
      // has its own block below.
      newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
    });
  });

  it('renders nothing when no directory is selected', () => {
    const { container } = renderOptions();
    expect(container).toBeEmptyDOMElement();
  });

  it('renders nothing for a non-git directory', async () => {
    useComposerStore.setState({ newSessionWorkdir: NON_GIT_DIR });
    renderOptions();

    // The repo probe resolves to `repo_root: null`, so the toggle never shows.
    await waitFor(() => {
      expect(screen.queryByTestId('worktree-options')).not.toBeInTheDocument();
    });
  });

  it('shows the worktree toggle for a git-repo directory', async () => {
    useComposerStore.setState({ newSessionWorkdir: MOCK_GIT_REPO_ROOT });
    renderOptions();

    expect(await screen.findByTestId('worktree-toggle')).toBeInTheDocument();
    // The start-point selector is hidden until the toggle is on.
    expect(screen.queryByTestId('worktree-start-point')).not.toBeInTheDocument();
  });

  it('reveals the start-point selector in "Other remote branch" mode when toggled on', async () => {
    // Dogfooding default: toggling worktree ON lands on the Other-remote-
    // branch picker (the `pending_remote_branch` sentinel) with the branch
    // list expanded, so the user picks a specific branch right away.
    useComposerStore.setState({ newSessionWorkdir: MOCK_GIT_REPO_ROOT });
    renderOptions();

    fireEvent.click(await screen.findByTestId('worktree-toggle'));

    expect(screen.getByTestId('worktree-start-point')).toBeInTheDocument();
    expect(screen.getByTestId('start-point-other')).toBeChecked();
    expect(screen.getByTestId('start-point-head')).not.toBeChecked();
    // The lazy remote-branch list is open immediately.
    expect(
      await screen.findByTestId('remote-branch-picker'),
    ).toBeInTheDocument();
    expect(useComposerStore.getState().newSessionWorktreeEnabled).toBe(true);
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });

  it('switching to HEAD then back to "other" returns to the pending-branch picker', async () => {
    // Switching radios across the default should land back on the pending
    // sentinel — no stale branch name slipping into the store on the way.
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
    });
    renderOptions();

    await screen.findByTestId('start-point-other');
    fireEvent.click(screen.getByTestId('start-point-head'));
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'head',
    });
    expect(
      screen.queryByTestId('remote-branch-picker'),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('start-point-other'));
    expect(screen.getByTestId('start-point-other')).toBeChecked();
    expect(
      await screen.findByTestId('remote-branch-picker'),
    ).toBeInTheDocument();
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });

  it('labels and selects the default-branch preset', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
    });
    renderOptions();

    const preset = await screen.findByTestId('start-point-default-branch');
    // The label uses the default branch from the git-repo probe ("main").
    expect(screen.getByTestId('worktree-start-point')).toHaveTextContent(
      'Latest main',
    );

    fireEvent.click(preset);
    // The first branch pick defaults to "use this branch" (use_remote_branch).
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
      name: 'main',
    });
  });

  it('lazily fetches and lists remote branches under "other"', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
      // Force a non-other initial state to assert the picker is gated on the
      // "other" choice — the default opens it immediately (covered above).
      newSessionWorktreeStartPoint: { kind: 'head' },
    });
    renderOptions();

    // The fetching branches query is gated: nothing is shown until "other".
    await screen.findByTestId('start-point-head');
    expect(
      screen.queryByTestId('remote-branch-picker'),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('start-point-other'));

    // The fetched remote branches list (mock: main/develop/release/1.0).
    const develop = await screen.findByTestId('remote-branch-develop');
    fireEvent.click(develop);
    // The first branch pick defaults to "use this branch" (use_remote_branch).
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
      name: 'develop',
    });
  });

  it('accepts a free-text remote branch name', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'remote_branch', name: '' },
    });
    renderOptions();

    fireEvent.click(await screen.findByTestId('start-point-other'));
    const input = await screen.findByTestId('remote-branch-input');
    fireEvent.change(input, { target: { value: 'feature/brand-new' } });

    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
      name: 'feature/brand-new',
    });
  });

  it('hides the use-vs-new choice for HEAD and shows it for a branch', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
      // Force HEAD to exercise the "HEAD is new-branch-only" path.
      newSessionWorktreeStartPoint: { kind: 'head' },
    });
    renderOptions();

    // HEAD is new-branch-only: no use-vs-new choice.
    await screen.findByTestId('start-point-head');
    expect(screen.queryByTestId('branch-mode')).not.toBeInTheDocument();

    // Picking the default-branch preset surfaces the use-vs-new choice,
    // defaulting to "use this branch".
    fireEvent.click(screen.getByTestId('start-point-default-branch'));
    expect(screen.getByTestId('branch-mode')).toBeInTheDocument();
    expect(screen.getByTestId('branch-mode-use')).toBeChecked();
    expect(screen.getByTestId('branch-mode-new')).not.toBeChecked();
  });

  it('emits remote_branch when "new branch from it" is chosen', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
    });
    renderOptions();

    // Picking a branch start-point lands on `use_remote_branch` (the new
    // default).
    fireEvent.click(await screen.findByTestId('start-point-default-branch'));
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
      name: 'main',
    });

    // Switching to "new branch from it" flips the kind.
    fireEvent.click(screen.getByTestId('branch-mode-new'));
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
      name: 'main',
    });
  });

  it('preserves the use-this-branch mode when switching the branch', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'use_remote_branch', name: 'main' },
    });
    renderOptions();

    // The "use this branch" mode reads as selected for the default-branch pick.
    await screen.findByTestId('branch-mode-use');
    expect(screen.getByTestId('branch-mode-use')).toBeChecked();

    // Switching to "other" and picking another branch keeps the use mode.
    fireEvent.click(screen.getByTestId('start-point-other'));
    const develop = await screen.findByTestId('remote-branch-develop');
    fireEvent.click(develop);
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
      name: 'develop',
    });
  });

  it('preserves the new-branch mode when switching the branch', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'remote_branch', name: 'main' },
    });
    renderOptions();

    // The explicit "new branch from it" mode reads as selected.
    await screen.findByTestId('branch-mode-new');
    expect(screen.getByTestId('branch-mode-new')).toBeChecked();

    // Switching to "other" and picking another branch keeps the new-branch
    // mode (the orthogonal use-vs-new state survives the start-point switch).
    fireEvent.click(screen.getByTestId('start-point-other'));
    const develop = await screen.findByTestId('remote-branch-develop');
    fireEvent.click(develop);
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
      name: 'develop',
    });
  });

  it('shows an inline error when the branches fetch fails', async () => {
    server.use(
      http.get('*/api/workdir/git/branches', () =>
        HttpResponse.json({ error: 'boom' }, { status: 400 }),
      ),
    );
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
    });
    renderOptions();

    fireEvent.click(await screen.findByTestId('start-point-other'));
    expect(await screen.findByTestId('remote-branch-error')).toHaveTextContent(
      'Could not fetch remote branches',
    );
  });
});

describe('WorktreeOptions with a PR-picked workdir', () => {
  // Seeded here rather than reset in an `afterEach`: React Testing Library's
  // auto-cleanup runs after this file's own hooks, so a store write on the way
  // out would land on a still-mounted component and trip React's act(...)
  // warning. Every block seeds the state it needs on entry, so no reset is
  // needed on exit.
  beforeEach(() => {
    // Exactly what `setNewSessionWorkdirFromPr` writes for the mock reviewer
    // fixture (see PRTab): the clone, `pr` provenance, and the head-branch
    // worktree.
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorkdirSource: {
        kind: 'pr',
        url: 'https://github.com/x7c1/delta/pull/174',
        number: 174,
        repo_owner: 'x7c1',
        repo_name: 'delta',
        head_ref: 'feat/repo-tab',
      },
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: {
        kind: 'use_remote_branch',
        name: 'feat/repo-tab',
      },
    });
  });

  it('locks the section to the PR head branch instead of offering the selector', async () => {
    renderOptions();

    // The section still renders (same chrome), but as a read-only summary.
    expect(await screen.findByTestId('worktree-options')).toHaveTextContent(
      'Start in an isolated git worktree',
    );
    const lock = screen.getByTestId('worktree-pr-lock');
    expect(lock).toHaveTextContent('feat/repo-tab');
    expect(lock).toHaveTextContent('PR #174’s head branch');

    // None of the choices the generic selector offers are reachable: the
    // session is for the PR, so its branch is not up for debate.
    expect(screen.queryByTestId('worktree-toggle')).not.toBeInTheDocument();
    expect(screen.queryByTestId('worktree-start-point')).not.toBeInTheDocument();
    expect(screen.queryByTestId('start-point-head')).not.toBeInTheDocument();
    expect(
      screen.queryByTestId('start-point-default-branch'),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId('start-point-other')).not.toBeInTheDocument();
    expect(screen.queryByTestId('branch-mode')).not.toBeInTheDocument();
    expect(
      screen.queryByTestId('remote-branch-picker'),
    ).not.toBeInTheDocument();
    // Nothing here can move the committed worktree request either.
    expect(useComposerStore.getState().newSessionWorktreeEnabled).toBe(true);
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
      name: 'feat/repo-tab',
    });
  });

  it('renders the lock without consulting the git probe', () => {
    // The probe is skipped for a PR pick — the clone is registered and the
    // branch is already decided — so the lock must not hang off its answer.
    // Seeding a path the mock reports as non-git pins that: the lock renders on
    // the first paint, where the `repo_root === null` bail-out of the directory
    // path would have rendered nothing.
    useComposerStore.setState({ newSessionWorkdir: NON_GIT_DIR });
    renderOptions();

    expect(screen.getByTestId('worktree-pr-lock')).toHaveTextContent(
      'feat/repo-tab',
    );
  });

  it('restores the full selector once a directory pick replaces the PR', async () => {
    renderOptions();
    await screen.findByTestId('worktree-pr-lock');

    // A Repository / Directory pick routes through `setNewSessionWorkdir`,
    // which stamps `directory` provenance — the lock unlocks in place.
    act(() => {
      useComposerStore.getState().setNewSessionWorkdir(MOCK_GIT_REPO_ROOT);
    });

    expect(await screen.findByTestId('worktree-toggle')).toBeInTheDocument();
    expect(screen.queryByTestId('worktree-pr-lock')).not.toBeInTheDocument();
    // The forced-on worktree went with the PR: the toggle reads off again.
    expect(screen.getByTestId('worktree-toggle')).not.toBeChecked();
  });
});
