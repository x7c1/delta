import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import {
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
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { WorkdirDialog } from './WorkdirDialog';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** The most-recent recent workdir (first row), which is pre-selected on open. */
const MOST_RECENT = '/home/dev/projects/delta';

function renderDialog(onClose = vi.fn(), { dismissable = true } = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <WorkdirDialog open onClose={onClose} dismissable={dismissable} />
      </ApiProvider>
    </QueryClientProvider>,
  );
  return { ...utils, onClose };
}

describe('WorkdirDialog', () => {
  beforeEach(() => {
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it('renders the dialog content when open', async () => {
    renderDialog();
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveAccessibleName('Where should this session run?');
    expect(screen.getByTestId('workdir-picker')).toBeInTheDocument();
  });

  it('explains why a directory is being chosen', async () => {
    renderDialog();
    const help = await screen.findByTestId('workdir-help');
    expect(help).toHaveTextContent(
      'Claude Code starts in this folder. Pick the project to work in.',
    );
  });

  it('pre-selects the most-recent directory as the candidate on open', async () => {
    renderDialog();

    // The first Recent row is highlighted (aria-pressed) without any click, and
    // Select is enabled so the user can confirm immediately. Looked up by its
    // stable `title` (full path); the visible label is abbreviated.
    const firstRow = await screen.findByTitle(MOST_RECENT);
    await waitFor(() => expect(firstRow).toHaveAttribute('aria-pressed', 'true'));
    expect(screen.getByTestId('workdir-confirm')).toBeEnabled();
  });

  it('marks directory rows with a folder icon', async () => {
    renderDialog();

    // The leading folder icon is decorative (aria-hidden) so it never changes a
    // row's accessible name, but it should render inside the row so the screen
    // reads as a directory picker.
    const firstRow = await screen.findByTitle(MOST_RECENT);
    expect(firstRow.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  });

  it('abbreviates the home directory to `~` while keeping the full path as the value', async () => {
    const { onClose } = renderDialog();

    // The most-recent row's title stays the absolute path, but its visible
    // label collapses the home directory (`/home/dev`) to `~`.
    const firstRow = await screen.findByTitle(MOST_RECENT);
    expect(firstRow).toHaveTextContent('~/projects/delta');
    expect(firstRow).not.toHaveTextContent('/home/dev');

    // Selecting still commits the absolute path, not the abbreviated label.
    fireEvent.click(screen.getByTestId('workdir-confirm'));
    expect(useComposerStore.getState().newSessionWorkdir).toBe(MOST_RECENT);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('commits the candidate and closes when Select is clicked', async () => {
    const { onClose } = renderDialog();

    // recent[0] is pre-selected; Select commits it.
    await screen.findByTitle(MOST_RECENT);
    fireEvent.click(screen.getByTestId('workdir-confirm'));

    expect(useComposerStore.getState().newSessionWorkdir).toBe(MOST_RECENT);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('selecting a different Recent row makes it the candidate', async () => {
    renderDialog();

    const otherRow = await screen.findByTitle('/home/dev/projects/website');
    fireEvent.click(otherRow);
    fireEvent.click(screen.getByTestId('workdir-confirm'));

    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/website',
    );
  });

  it('closes without committing when Cancel is clicked', async () => {
    const { onClose } = renderDialog();

    await screen.findByTitle(MOST_RECENT);
    fireEvent.click(screen.getByTestId('workdir-cancel'));

    // Cancel never commits, even though a recent row was pre-selected.
    expect(useComposerStore.getState().newSessionWorkdir).toBeNull();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('hides Cancel but still commits via Select when not dismissable', async () => {
    const { onClose } = renderDialog(vi.fn(), { dismissable: false });

    // The only way out is to choose a directory: Cancel is absent.
    await screen.findByTitle(MOST_RECENT);
    expect(screen.queryByTestId('workdir-cancel')).not.toBeInTheDocument();

    // Select still commits the pre-selected candidate and closes.
    fireEvent.click(screen.getByTestId('workdir-confirm'));
    expect(useComposerStore.getState().newSessionWorkdir).toBe(MOST_RECENT);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('descends, ascends, and the browsed directory becomes the candidate', async () => {
    renderDialog();

    // Default browse lists $HOME (/home/dev) with its subdirectories. The
    // current-directory label collapses home to `~` and spells it out as
    // `~ (HOME)` so the bare tilde is not overlooked (title keeps the full
    // path).
    await waitFor(() => {
      expect(screen.getByTestId('workdir-use-current')).toHaveAttribute(
        'title',
        '/home/dev',
      );
    });
    expect(screen.getByTestId('workdir-use-current')).toHaveTextContent(
      'Use this directory: ~ (HOME)',
    );

    // Descend into projects/. Navigating makes the browsed dir the candidate,
    // dropping the recent pre-selection.
    fireEvent.click(screen.getByRole('button', { name: 'projects/' }));
    await waitFor(() => {
      expect(screen.getByTestId('workdir-use-current')).toHaveAttribute(
        'title',
        '/home/dev/projects',
      );
    });
    expect(screen.getByTestId('workdir-use-current')).toHaveTextContent(
      'Use this directory: ~/projects',
    );
    // A non-home directory keeps the plain abbreviation — no `(HOME)` label.
    expect(screen.getByTestId('workdir-use-current')).not.toHaveTextContent(
      '(HOME)',
    );
    expect(screen.getByTestId('workdir-use-current')).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    // The recent row (now showing the abbreviated `~/projects/delta`) is no
    // longer the candidate. Scope the lookup to the Recent section since the
    // `delta/` browse entry shares the same absolute path/title.
    expect(
      within(screen.getByTestId('workdir-recent')).getByTitle(MOST_RECENT),
    ).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByRole('button', { name: 'delta/' })).toBeInTheDocument();

    // Ascend via the ".." entry back to $HOME; that dir is now the candidate.
    fireEvent.click(screen.getByTestId('workdir-parent'));
    await waitFor(() => {
      expect(screen.getByTestId('workdir-use-current')).toHaveAttribute(
        'title',
        '/home/dev',
      );
    });
    expect(screen.getByTestId('workdir-use-current')).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    // Navigation alone made it the candidate: Select commits it without ever
    // clicking "Use this directory".
    fireEvent.click(screen.getByTestId('workdir-confirm'));
    expect(useComposerStore.getState().newSessionWorkdir).toBe('/home/dev');
  });

  it('keeps Select disabled until a browse pick when Recent is empty', async () => {
    // No recent list: nothing is pre-selected and Select stays disabled.
    server.use(
      http.get('*/api/workdir/recent', () =>
        HttpResponse.json({ workdirs: [] }, { status: 200 }),
      ),
    );
    renderDialog();

    // Browse renders; the Recent section is omitted.
    await screen.findByTestId('workdir-browse');
    expect(screen.queryByTestId('workdir-recent')).not.toBeInTheDocument();
    expect(screen.getByTestId('workdir-confirm')).toBeDisabled();

    // Picking the browsed directory enables Select.
    fireEvent.click(await screen.findByTestId('workdir-use-current'));
    expect(screen.getByTestId('workdir-confirm')).toBeEnabled();
  });

  it('shows an inline error and offers a way back when a listing is forbidden', async () => {
    server.use(
      http.get('*/api/workdir/list', () =>
        HttpResponse.json({ error: 'permission denied' }, { status: 403 }),
      ),
    );
    renderDialog();

    const error = await screen.findByTestId('workdir-error');
    expect(error).toHaveTextContent('Permission denied');
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
    renderDialog();

    const error = await screen.findByTestId('workdir-error');
    expect(error).toHaveTextContent('could not be opened');
  });

  it('hides the Recent section and pre-selects nothing when recent fails', async () => {
    server.use(
      http.get('*/api/workdir/recent', () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 }),
      ),
    );
    renderDialog();

    await waitFor(() => {
      expect(screen.getByTestId('workdir-browse')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('workdir-recent')).not.toBeInTheDocument();
    // Nothing pre-selected: Select is disabled until a browse pick.
    expect(screen.getByTestId('workdir-confirm')).toBeDisabled();
  });
});
