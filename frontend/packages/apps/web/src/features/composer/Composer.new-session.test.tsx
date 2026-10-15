/**
 * Sending from the new-session screen: the gates on Send, and the body of
 * the new-session request.
 *
 * Shared fixtures live in Composer.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { Composer } from './Composer';
import { resetComposerState, server } from './Composer.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('Composer', () => {
  beforeEach(resetComposerState);

  it('targets a new session when in new-session mode', async () => {
    // A new session needs a chosen directory before Send is enabled.
    useComposerStore.setState({ newSessionWorkdir: '/home/dev' });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // The accepted spawn is tracked under the REAL ids the server returned, so
    // the workspace can focus the new session directly and the chip survives
    // through the first turn.
    await waitFor(() => {
      const spawns = useLiveStore.getState().spawns;
      expect(spawns).toHaveLength(1);
      expect(spawns[0].text).toBe('start fresh');
      expect(spawns[0].sessionId).not.toBe('');
      expect(spawns[0].status).toBe('spawning');
    });
    const locals = Object.values(useLiveStore.getState().localSends);
    expect(locals).toHaveLength(1);
    expect(locals[0].text).toBe('start fresh');
  });

  it('disables Send while the worktree toggle is on but no branch is picked yet', async () => {
    // Dogfooding default: toggling worktree ON lands on the Other-remote-
    // branch picker (the `pending_remote_branch` sentinel) so the user picks
    // a specific branch. The backend rejects a worktree request without a
    // concrete branch, so Send must stay disabled until the user picks one
    // — picking unblocks Send.
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'pending_remote_branch' },
    });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();

    // Picking a concrete branch unblocks Send.
    act(() => {
      useComposerStore.setState({
        newSessionWorktreeStartPoint: {
          kind: 'use_remote_branch',
          name: 'develop',
        },
      });
    });
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
    });
  });

  it('Cmd+Enter is a no-op for a new session when no workdir is selected', async () => {
    // Regression: the Send button gates a new session on `newSessionWorkdir`,
    // but the keyboard shortcut used to bypass that gate. Without a workdir
    // the backend would spawn in its default scratch dir and the watchdog
    // would later report "session failed to start", so Cmd+Enter must obey
    // the same disabled rule as the button.
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    fireEvent.keyDown(textarea, { key: 'Enter', metaKey: true });
    fireEvent.keyDown(textarea, { key: 'Enter', ctrlKey: true });

    // No spawn or local send was queued — the shortcut silently did nothing,
    // matching the disabled Send button.
    expect(useLiveStore.getState().spawns).toHaveLength(0);
    expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(0);
  });

  it('Cmd+Enter submits a new session once a workdir is selected', async () => {
    // Pair to the guard test above: once the gate is satisfied, the shortcut
    // must still work (we are guarding, not removing it).
    useComposerStore.setState({ newSessionWorkdir: '/home/dev' });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    fireEvent.keyDown(textarea, { key: 'Enter', metaKey: true });

    await waitFor(() => {
      expect(useLiveStore.getState().spawns).toHaveLength(1);
    });
    expect(useLiveStore.getState().spawns[0].text).toBe('start fresh');
  });

  it('disables Send for a new session until a workdir is selected', async () => {
    // Default state: no directory chosen. Selection is mandatory for a new
    // session, so Send stays disabled even with text entered.
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start fresh' } });
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();

    // Once a directory is selected, Send becomes enabled.
    act(() => {
      useComposerStore.setState({ newSessionWorkdir: '/home/dev' });
    });
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
    });
  });

  /**
   * Render the new-session composer and capture the body of the `POST
   * /api/sends` it fires, so a test can assert exactly which fields are sent.
   */
  function renderNewSessionAndCaptureBody(): { read: () => unknown } {
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
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );
    return { read: () => captured };
  }

  // A launch option the provider's adapter will not apply is refused with a
  // stable code and a message that names WHICH selection is wrong — the field
  // Delta owns, or the `config` key path two selected rows disagree about. That
  // message is the only account the user gets, so the chip has to keep it
  // rather than collapse it into "the session failed to start".
  it('keeps a refused launch option message on the failed chip', async () => {
    const message =
      'launch option rejected: the selected `config` options disagree: ' +
      '`model_reasoning_effort` is set to both "high" and "low"';
    server.use(
      http.post('*/api/sends', () =>
        HttpResponse.json(
          { error: message, code: 'launch_option_rejected' },
          { status: 400 },
        ),
      ),
    );
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionLaunchOptionIds: [3, 1],
    });
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
          <Composer mode={{ kind: 'new-session' }} />
        </ApiProvider>
      </QueryClientProvider>,
    );

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start with options' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      const sending = useLiveStore.getState().sending;
      expect(sending[0]?.status).toBe('failed');
      expect(sending[0]?.reason).toBe(message);
    });
  });

  it('includes the selected workdir on a new-session send', async () => {
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start in delta' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start in delta',
        workdir: '/home/dev/projects/delta',
      });
    });
    // The picker selection is preserved after submit; TranscriptPane resets it
    // when the new-session state is left.
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
  });

  it('includes the selected launch options on a new-session send, in order', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      // Selection order (not ascending) so the test pins order preservation.
      newSessionLaunchOptionIds: [3, 1],
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start with options' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start with options',
        workdir: '/home/dev/projects/delta',
        launch_option_ids: [3, 1],
      });
    });
    // The picker selection is preserved after submit; TranscriptPane resets it
    // when the new-session state is left.
    expect(useComposerStore.getState().newSessionWorkdir).toBe(
      '/home/dev/projects/delta',
    );
    expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
      3, 1,
    ]);
  });

  it('includes worktree with the chosen start-point when the toggle is on', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: { kind: 'remote_branch', name: 'develop' },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start in a worktree' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start in a worktree',
        workdir: '/home/dev/projects/delta',
        worktree: { start_point: { kind: 'remote_branch', name: 'develop' } },
      });
    });
  });

  it('includes the use_remote_branch start-point when "use this branch" is chosen', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: true,
      newSessionWorktreeStartPoint: {
        kind: 'use_remote_branch',
        name: 'develop',
      },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'work on develop directly' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'work on develop directly',
        workdir: '/home/dev/projects/delta',
        worktree: {
          start_point: { kind: 'use_remote_branch', name: 'develop' },
        },
      });
    });
  });

  it('omits worktree when the toggle is off', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorktreeEnabled: false,
      // A non-default start-point must NOT leak when the toggle is off.
      newSessionWorktreeStartPoint: { kind: 'remote_branch', name: 'develop' },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'no worktree' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'no worktree',
        workdir: '/home/dev/projects/delta',
      });
    });
  });

  it('omits launch_option_ids when no launch options are selected', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionLaunchOptionIds: [],
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'no options' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'no options',
        workdir: '/home/dev/projects/delta',
      });
    });
  });

  it('includes the PR number when the workdir was picked from a pull request', async () => {
    // Only the PR tab sets a `pr` workdir source, so it is exactly the signal
    // for "this session is being opened from a pull request". The backend
    // snapshots the number and the navigator card renders it.
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionWorkdirSource: {
        kind: 'pr',
        url: 'https://github.com/x7c1/delta/pull/138',
        number: 138,
        repo_owner: 'x7c1',
        repo_name: 'delta',
        head_ref: 'feat/thing',
      },
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'resume PR work' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'resume PR work',
        workdir: '/home/dev/projects/delta',
        pull_request_number: 138,
      });
    });
  });

  it('omits the PR number after the workdir is re-picked as a directory', async () => {
    // A directory pick resets the provenance, so a stale PR must not ride along
    // on a send that no longer has anything to do with it.
    useComposerStore.setState({
      newSessionWorkdirSource: {
        kind: 'pr',
        url: 'https://github.com/x7c1/delta/pull/138',
        number: 138,
        repo_owner: 'x7c1',
        repo_name: 'delta',
        head_ref: 'feat/thing',
      },
    });
    act(() => {
      useComposerStore
        .getState()
        .setNewSessionWorkdir('/home/dev/projects/other');
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'plain start' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'plain start',
        workdir: '/home/dev/projects/other',
      });
    });
  });

  it('sends provider: "codex" on a new-session send when Codex is selected', async () => {
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionProvider: 'codex',
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start on codex' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start on codex',
        workdir: '/home/dev/projects/delta',
        provider: 'codex',
      });
    });
  });

  it('omits provider on a new-session send for the Claude default', async () => {
    // The backend defaults an omitted provider to Claude, so the composer omits
    // it for the default to keep the Claude send byte-for-byte identical to
    // today's — this test pins that intentional omission.
    useComposerStore.setState({
      newSessionWorkdir: '/home/dev/projects/delta',
      newSessionProvider: 'claude',
    });
    const { read } = renderNewSessionAndCaptureBody();

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'start on claude' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => {
      expect(read()).toEqual({
        new_session: true,
        text: 'start on claude',
        workdir: '/home/dev/projects/delta',
      });
    });
  });
});
