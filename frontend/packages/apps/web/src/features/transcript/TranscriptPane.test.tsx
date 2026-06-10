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
import type { MessagesResponse } from '@delta/model';
import {
  MAIN_THREAD_ID,
  SESSION_ID,
  createHandlers,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { TranscriptPane } from './TranscriptPane';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPane(threads = mockThreads) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const main = threads.find((t) => t.id === MAIN_THREAD_ID)!;
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane threads={threads} activeThread={main} readOnly={false} />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('TranscriptPane', () => {
  beforeEach(() => {
    useNavStore.setState({
      activeThreadId: MAIN_THREAD_ID,
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: null,
    });
    useLiveStore.setState({
      pending: [],
      permission: {},
      externalInput: {},
      resumeUnavailable: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
  });

  function renderNewSessionPane(
    threads = mockThreads,
    { workdirMandatory = false } = {},
  ) {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const client = new ApiClient({ baseUrl: 'http://localhost' });
    return render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={client}>
          <TranscriptPane
            threads={threads}
            activeThread={null}
            readOnly={false}
            newSession
            workdirMandatory={workdirMandatory}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );
  }

  it('renders messages fetched from the mocked REST API', async () => {
    renderPane();

    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );
    // Assistant Markdown text is foregrounded.
    expect(screen.getByText(/change between two states/)).toBeInTheDocument();
    // The breadcrumb shows the current location.
    expect(
      screen.getByRole('navigation', { name: 'Breadcrumb' }),
    ).toHaveTextContent('main');
  });

  it('hides the breadcrumb until the session has branched', async () => {
    // A main-only session (no sub-threads) should not show a lone "main"
    // breadcrumb, which reads as abrupt with no tree to place it in.
    const mainOnly = mockThreads.filter((t) => t.parent_thread_id === null);
    renderPane(mainOnly);

    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole('navigation', { name: 'Breadcrumb' }),
    ).not.toBeInTheDocument();
  });

  it('renders a branch chip where a child thread sprouts', async () => {
    renderPane();

    await waitFor(() =>
      expect(screen.getByText(/delta etymology/)).toBeInTheDocument(),
    );
  });

  it('does not render non-conversational (system/other) lines', async () => {
    // The transcript persists these lines but the view must skip them.
    server.use(
      http.get('*/api/threads/:id/messages', () => {
        const body: MessagesResponse = {
          messages: [
            {
              uuid: 'm-user',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'user',
              linear_parent_uuid: null,
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 0,
              content_text: 'hello there',
              content: [{ type: 'text', text: 'hello there' }],
              created_at: '2026-01-01T00:00:01Z',
            },
            {
              uuid: 'm-system',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'system',
              linear_parent_uuid: 'm-user',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: 'SECRET SYSTEM NOISE',
              content: [{ type: 'text', text: 'SECRET SYSTEM NOISE' }],
              created_at: '2026-01-01T00:00:02Z',
            },
            {
              uuid: 'm-other',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'other',
              linear_parent_uuid: 'm-system',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 2,
              content_text: 'OTHER NOISE',
              content: [{ type: 'text', text: 'OTHER NOISE' }],
              created_at: '2026-01-01T00:00:03Z',
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane();

    await waitFor(() =>
      expect(screen.getByText('hello there')).toBeInTheDocument(),
    );
    expect(screen.queryByText('SECRET SYSTEM NOISE')).not.toBeInTheDocument();
    expect(screen.queryByText('OTHER NOISE')).not.toBeInTheDocument();
  });

  it('drops the composer and shows the cannot-resume notice for a resume-unavailable session', async () => {
    // A session whose transcript is gone can never be resumed, so every send or
    // branch would just fail: the input is removed entirely and the session is a
    // read-only viewer with a pinned notice. The history stays readable.
    useLiveStore.setState({
      pending: [],
      externalInput: {},
      resumeUnavailable: { [SESSION_ID]: true },
    });

    renderPane();

    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );
    expect(
      screen.getByTestId('resume-unavailable-notice'),
    ).toBeInTheDocument();
    // No input affordance: neither the textarea nor the Send button is rendered.
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Send' }),
    ).not.toBeInTheDocument();
  });

  it('shows the external-input notice for the focused thread (pinned above the input)', async () => {
    useLiveStore.setState({
      pending: [],
      resumeUnavailable: {},
      externalInput: {
        [SESSION_ID]: { threadId: MAIN_THREAD_ID, prompt: 'typed in the pane', at: 0 },
      },
    });

    renderPane();

    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );
    const notice = screen.getByTestId('external-input-notice');
    expect(notice).toHaveTextContent('typed in the pane');
  });

  it('dismisses the external-input notice via its Dismiss button', async () => {
    useLiveStore.setState({
      pending: [],
      resumeUnavailable: {},
      externalInput: {
        [SESSION_ID]: { threadId: MAIN_THREAD_ID, prompt: 'typed in the pane', at: 0 },
      },
    });

    renderPane();

    const notice = await screen.findByTestId('external-input-notice');
    fireEvent.click(
      within(notice).getByRole('button', { name: 'Dismiss' }),
    );

    expect(
      screen.queryByTestId('external-input-notice'),
    ).not.toBeInTheDocument();
    expect(useLiveStore.getState().externalInput).toEqual({});
  });

  it('does not flash the permission notice when the request resolves within the debounce window', async () => {
    vi.useFakeTimers();
    try {
      useLiveStore.setState({
        pending: [],
        externalInput: {},
        resumeUnavailable: {},
        permission: { [SESSION_ID]: { requestId: 7, toolName: 'Bash' } },
      });

      renderPane();

      // Within the debounce window the notice has not painted yet.
      act(() => {
        vi.advanceTimersByTime(100);
      });
      expect(
        screen.queryByTestId('permission-notice'),
      ).not.toBeInTheDocument();

      // The request resolves (auto-approved tool's tool_result ingested) before
      // the window elapses, so the notice must never appear.
      act(() => {
        useLiveStore.getState().applyEvent({
          kind: 'permission_resolved',
          session_id: SESSION_ID,
          request_id: 7,
        });
      });
      act(() => {
        vi.advanceTimersByTime(1000);
      });
      expect(
        screen.queryByTestId('permission-notice'),
      ).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows the permission notice once it outlasts the debounce window', async () => {
    vi.useFakeTimers();
    try {
      useLiveStore.setState({
        pending: [],
        externalInput: {},
        resumeUnavailable: {},
        permission: { [SESSION_ID]: { requestId: 7, toolName: 'Bash' } },
      });

      renderPane();

      // A genuine pending prompt has no resolution; once the window elapses the
      // notice renders as normal.
      act(() => {
        vi.advanceTimersByTime(400);
      });
      const notice = screen.getByTestId('permission-notice');
      expect(notice).toHaveTextContent('Permission requested: Bash');
    } finally {
      vi.useRealTimers();
    }
  });

  it('auto-opens the workdir dialog on entering the new-session state', async () => {
    renderNewSessionPane();

    // The modal opens without any user action, with the most-recent directory
    // pre-selected so the user can confirm immediately.
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    const firstRow = await screen.findByTitle('/home/dev/projects/delta');
    await waitFor(() =>
      expect(firstRow).toHaveAttribute('aria-pressed', 'true'),
    );
  });

  it('makes the workdir dialog non-dismissable when workdirMandatory', async () => {
    // First run (no sessions to fall back to): the picker is mandatory, so there
    // is no Cancel button and Esc/backdrop do not close it.
    renderNewSessionPane(mockThreads, { workdirMandatory: true });

    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(screen.queryByTestId('workdir-cancel')).not.toBeInTheDocument();

    fireEvent.keyDown(document, { key: 'Escape' });
    fireEvent.click(screen.getByTestId('dialog-backdrop'));

    // The dialog is still open and the new-session intent is intact.
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('shows a chip with an edit affordance once a directory is selected', async () => {
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    const chip = screen.getByTestId('workdir-chip');
    // The path label collapses home to `~` once $HOME is known, while the
    // full path is preserved in the title for hover.
    await waitFor(() =>
      expect(chip).toHaveTextContent('Start in:~/projects/delta'),
    );
    expect(
      within(chip).getByTitle('/home/dev/projects/delta'),
    ).toBeInTheDocument();
    // The ✎ reopens the dialog rather than clearing the (mandatory) selection.
    expect(
      within(chip).getByRole('button', { name: 'Change working directory' }),
    ).toBeInTheDocument();
  });

  it('reopens the picker from the chip ✎ without resetting the selection', async () => {
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    // The picker starts closed (a directory is already selected, so the
    // auto-open effect does not fire).
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    fireEvent.click(
      within(screen.getByTestId('workdir-chip')).getByRole('button', {
        name: 'Change working directory',
      }),
    );

    // The ✎ opens the picker via openWorkdirDialog (no reset), so the chosen
    // directory is still in the store while editing.
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
  });

  it('closes the picker and shows no chip after cancelling without selecting (no session to return to)', async () => {
    // No previous session is recorded (the empty initial screen), so dismissing
    // the picker is a no-op: new-session stays as the mandatory default.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: null,
    });
    renderNewSessionPane();

    // Cancel the auto-opened modal without committing a directory.
    fireEvent.click(await screen.findByTestId('workdir-cancel'));

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    // No selection: no chip is shown and Send stays disabled. Reopening the
    // picker is now done from the navigator's "New" button, not a center button.
    expect(screen.queryByTestId('workdir-chip')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    // Stays in new-session (nowhere to return to).
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('returns to the previously-focused session when the picker is dismissed without a selection', async () => {
    // The user was on a real session before clicking "New", so dismissing the
    // picker without choosing a directory cancels the new-session intent and
    // restores that session.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    renderNewSessionPane();

    fireEvent.click(await screen.findByTestId('workdir-cancel'));

    await waitFor(() =>
      expect(useNavStore.getState().focusedSessionId).toBe(SESSION_ID),
    );
    expect(useNavStore.getState().preNewSessionFocus).toBeNull();
  });

  it('stays in new-session when a directory has been selected (dismiss does not cancel)', async () => {
    // A previous session is recorded, but a directory is already chosen, so
    // dismissing the picker (e.g. the ✎-reopen-then-close path) must NOT cancel
    // the new-session intent.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    // Reopen via the chip's edit affordance, then dismiss.
    fireEvent.click(
      within(screen.getByTestId('workdir-chip')).getByRole('button', {
        name: 'Change working directory',
      }),
    );
    fireEvent.click(await screen.findByTestId('workdir-cancel'));

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    // Still in new-session: a directory is selected, so the cancel was skipped.
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });
});
