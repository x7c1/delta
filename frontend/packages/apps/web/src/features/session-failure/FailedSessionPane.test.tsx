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
import { http, HttpResponse } from 'msw';
import { ApiClient, queryKeys } from '@delta/api-client';
import type { Send, Session, SessionListItem } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore, type SpawnItem } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { FailedSessionPane } from './FailedSessionPane';

const FAILED_ID = 'sess-failed';

const server = setupServer();
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function failedSession(overrides: Partial<Session> = {}): SessionListItem {
  return {
    session: {
      id: FAILED_ID,
      cwd: '/work/delta',
      transcript_path: '',
      title: null,
      status: 'failed',
      created_at: '2026-01-01T00:00:00Z',
      branch_at_launch: 'main',
      repo_root: '/work/delta',
      repository_display_name: 'x7c1/delta',
      provider: 'claude',
      provider_session_id: null,
      provider_thread_id: null,
      pull_request_number: null,
      failure_reason: 'git error: worktree add failed',
      ...overrides,
    },
    open: false,
    main_thread_id: 7,
    last_activity_at: null,
  };
}

function undelivered(text: string, id = 1): Send {
  return {
    id,
    session_id: FAILED_ID,
    thread_id: 7,
    semantic_parent_uuid: null,
    text,
    locator_quote: null,
    status: 'dispatched',
    matched_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    held_at: null,
  };
}

function trackedFailure(overrides: Partial<SpawnItem> = {}): SpawnItem {
  return {
    sessionId: FAILED_ID,
    threadId: 7,
    text: 'start a new session',
    firstSendId: 1,
    workdir: '/work/delta',
    launchOptionIds: [2, 5],
    provider: 'claude',
    worktree: null,
    pullRequestNumber: null,
    status: 'failed',
    ...overrides,
  };
}

function renderPane(item = failedSession(), sends: Send[] = []) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(queryKeys.sessionSends(item.session.id), { sends });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <FailedSessionPane item={item} />
      </ApiProvider>
    </QueryClientProvider>,
  );
  return queryClient;
}

beforeEach(() => {
  useLiveStore.setState({ spawns: [], sending: [], localSends: {} });
  useNavStore.setState({ focusedSessionId: FAILED_ID });
  server.use(
    http.get('*/api/sessions/:id/sends', () => HttpResponse.json({ sends: [] })),
  );
});

describe('FailedSessionPane', () => {
  it('explains the failure and shows what was never delivered', () => {
    useLiveStore.setState({ spawns: [trackedFailure()] });
    renderPane(failedSession(), [
      undelivered('start a new session', 1),
      undelivered('and one more while it started', 2),
    ]);

    expect(screen.getByText('This session never started.')).toBeInTheDocument();
    // The reason comes off the ROW, so it is there after a reload too.
    expect(screen.getByTestId('failed-session-reason')).toHaveTextContent(
      'git error: worktree add failed',
    );
    expect(
      screen.getAllByTestId('failed-session-prompt').map((el) => el.textContent),
    ).toEqual(['start a new session', 'and one more while it started']);
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeInTheDocument();
  });

  it('says so plainly when Delta never heard why', () => {
    // The watchdog-shaped endings observe only silence. Leaving the question
    // hanging would read as a pane that failed to load its own explanation.
    renderPane(failedSession({ failure_reason: null }));

    expect(screen.getByTestId('failed-session-reason')).toHaveTextContent(
      /did not hear why/i,
    );
  });

  it('words a cancel as a cancel, not as a breakage', () => {
    // The user closed a session that was still starting. Nothing broke, so the
    // pane states what happened instead of alarming — and the persisted reason
    // names the close.
    useLiveStore.setState({ spawns: [trackedFailure({ cancelled: true })] });
    renderPane(failedSession({ failure_reason: 'closed while starting' }));

    expect(screen.getByText('This launch was cancelled.')).toBeInTheDocument();
    expect(screen.getByText('cancelled')).toBeInTheDocument();
    expect(screen.queryByText('failed')).toBeNull();
    expect(screen.getByTestId('failed-session-reason')).toHaveTextContent(
      'closed while starting',
    );
  });

  it('offers Remove alone when this browser does not hold the launch', () => {
    // Retry re-attempts the IDENTICAL launch, and the launch-option ids and
    // worktree request live only in this browser's spawn registry — a reload,
    // or another tab, has neither. Offering a button that would quietly launch
    // something else configured differently is worse than not offering it.
    renderPane();

    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeInTheDocument();
  });

  it('says that Retry keeps only the first message, when there are more', () => {
    // Retry re-sends the launch's own first prompt and removes the row, which
    // takes everything typed behind it with it. Nothing hands that text back
    // any more, so this screen is the last place it can be read — pressing
    // Retry without being told would destroy it silently.
    useLiveStore.setState({ spawns: [trackedFailure()] });
    renderPane(failedSession(), [
      undelivered('start a new session', 1),
      undelivered('and one more while it started', 2),
    ]);

    expect(
      screen.getByTestId('failed-session-undelivered-fate'),
    ).toHaveTextContent(/Retry re-sends the first message only/);
  });

  it('says what Remove destroys when Retry is not on offer', () => {
    // Without the tracked launch there is no Retry, so the single undelivered
    // message has no path out either: Remove is the only action and it takes
    // the text with it. "Remove" names the session, not the text, so the loss
    // has to be said.
    renderPane(failedSession(), [undelivered('never made it out', 1)]);

    expect(
      screen.getByTestId('failed-session-undelivered-fate'),
    ).toHaveTextContent('Remove deletes this session, and this text with it');
  });

  it('says nothing about lost text when Retry re-sends all there is', () => {
    // One undelivered message is the launch's own prompt: Retry re-sends
    // exactly it, so there is nothing to warn about.
    useLiveStore.setState({ spawns: [trackedFailure()] });
    renderPane(failedSession(), [undelivered('start a new session', 1)]);
    expect(screen.queryByTestId('failed-session-undelivered-fate')).toBeNull();
  });

  it('Remove deletes the session and drops its tracked launch', async () => {
    const deleted: string[] = [];
    server.use(
      http.delete('*/api/sessions/:id', ({ params }) => {
        deleted.push(String(params.id));
        return new HttpResponse(null, { status: 204 });
      }),
    );
    useLiveStore.setState({ spawns: [trackedFailure()] });
    renderPane();

    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    await waitFor(() => expect(deleted).toEqual([FAILED_ID]));
    await waitFor(() => expect(useLiveStore.getState().spawns).toEqual([]));
  });

  it('Retry re-launches the identical configuration and removes this row', async () => {
    const bodies: unknown[] = [];
    server.use(
      http.post('*/api/sends', async ({ request }) => {
        bodies.push(await request.json());
        return HttpResponse.json({
          send: {
            ...undelivered('start a new session', 9),
            session_id: 'sess-retried',
            thread_id: 11,
            status: 'queued',
          },
        });
      }),
      http.delete('*/api/sessions/:id', () => new HttpResponse(null, { status: 204 })),
    );
    useLiveStore.setState({ spawns: [trackedFailure()] });
    renderPane();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    // The same first prompt, the same directory, the same launch options.
    expect(bodies[0]).toMatchObject({
      new_session: true,
      text: 'start a new session',
      workdir: '/work/delta',
      launch_option_ids: [2, 5],
    });
    // Focus moves to the new-session screen first, which is where the
    // workspace hands a fresh spawn's focus over from — so the retry lands the
    // user in the session it starts rather than on the row it replaces.
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    // And the failed row does not linger beside the session that replaced it.
    await waitFor(() =>
      expect(
        useLiveStore
          .getState()
          .spawns.some((spawn) => spawn.sessionId === FAILED_ID),
      ).toBe(false),
    );
  });
});
