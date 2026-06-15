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
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, MOCK_GIT_REPO_ROOT } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import {
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

  it('reveals the start-point selector with HEAD as the default when toggled on', async () => {
    useComposerStore.setState({ newSessionWorkdir: MOCK_GIT_REPO_ROOT });
    renderOptions();

    fireEvent.click(await screen.findByTestId('worktree-toggle'));

    expect(screen.getByTestId('worktree-start-point')).toBeInTheDocument();
    expect(screen.getByTestId('start-point-head')).toBeChecked();
    expect(useComposerStore.getState().newSessionWorktreeEnabled).toBe(true);
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'head',
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
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
      name: 'main',
    });
  });

  it('lazily fetches and lists remote branches under "other"', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
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
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
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
    });
    renderOptions();

    // HEAD is new-branch-only: no use-vs-new choice.
    await screen.findByTestId('start-point-head');
    expect(screen.queryByTestId('branch-mode')).not.toBeInTheDocument();

    // Picking the default-branch preset surfaces the use-vs-new choice,
    // defaulting to "new branch from it".
    fireEvent.click(screen.getByTestId('start-point-default-branch'));
    expect(screen.getByTestId('branch-mode')).toBeInTheDocument();
    expect(screen.getByTestId('branch-mode-new')).toBeChecked();
    expect(screen.getByTestId('branch-mode-use')).not.toBeChecked();
  });

  it('emits use_remote_branch when "use this branch" is chosen', async () => {
    useComposerStore.setState({
      newSessionWorkdir: MOCK_GIT_REPO_ROOT,
      newSessionWorktreeEnabled: true,
    });
    renderOptions();

    fireEvent.click(await screen.findByTestId('start-point-default-branch'));
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
      name: 'main',
    });

    fireEvent.click(screen.getByTestId('branch-mode-use'));
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
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
