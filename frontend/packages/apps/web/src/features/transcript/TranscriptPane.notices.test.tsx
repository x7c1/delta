/**
 * The notices around the conversation: closed, cannot-resume, external input,
 * parked send, running subagents, and the permission notice.
 *
 * Shared fixtures live in TranscriptPane.testkit.tsx.
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
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  SESSION_ID,
  mockThreads,
} from '@delta/api-mocks';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import {
  renderPane,
  resetTranscriptPaneState,
  server,
} from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane', () => {
  beforeEach(resetTranscriptPaneState);

  it('says a closed session is closed and that sending resumes it', async () => {
    renderPane(mockThreads, MAIN_THREAD_ID, { readOnly: true });

    expect(await screen.findByTestId('readonly-notice')).toHaveTextContent(
      'This session is closed. Sending a message resumes it.',
    );
  });

  it('shows why Delta closed a session in the closed notice', async () => {
    // A session Delta closed on its own carries the reason on its row; the
    // notice says it in place of the plain wording, so the user knows why the
    // session closed and that a send brings it back.
    renderPane(mockThreads, MAIN_THREAD_ID, {
      readOnly: true,
      closedReason: 'Delta moved to a new port; resume to reconnect',
    });

    expect(await screen.findByTestId('readonly-notice')).toHaveTextContent(
      'Delta moved to a new port; resume to reconnect. Sending a message resumes it.',
    );
  });

  it('drops the composer and shows the cannot-resume notice for a resume-unavailable session', async () => {
    // A session whose transcript is gone can never be resumed, so every send or
    // branch would just fail: the input is removed entirely and the session is a
    // read-only viewer with a pinned notice. The history stays readable.
    useLiveStore.setState({
      notices: { [SESSION_ID]: [{ kind: 'resume_unavailable' }] },
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
      notices: {
        [SESSION_ID]: [
          {
            kind: 'external_input',
            threadId: MAIN_THREAD_ID,
            prompt: 'typed in the pane',
            at: 0,
          },
        ],
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
      notices: {
        [SESSION_ID]: [
          {
            kind: 'external_input',
            threadId: MAIN_THREAD_ID,
            prompt: 'typed in the pane',
            at: 0,
          },
        ],
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
    expect(useLiveStore.getState().notices).toEqual({});
  });

  it('explains a parked send without repeating its text, and dismisses the notice', async () => {
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [{ kind: 'send_parked', sendId: 42, at: 0 }],
      },
    });

    renderPane();

    const notice = await screen.findByTestId('send-parked-notice');
    // The message itself is back in the queue (the pending strip renders it
    // with Send and Cancel), so the notice only says why it is waiting — it
    // must not tell the user to copy a text it no longer holds.
    expect(notice).toHaveTextContent('never reached the session');
    expect(notice).toHaveTextContent('waiting in the queue');
    expect(notice).not.toHaveTextContent(/copy it/i);

    fireEvent.click(within(notice).getByRole('button', { name: 'Dismiss' }));

    expect(screen.queryByTestId('send-parked-notice')).not.toBeInTheDocument();
    expect(useLiveStore.getState().notices).toEqual({});
  });

  it('shows the running-subagent indicator for the active thread, indented to align with tool calls', async () => {
    useLiveStore.setState({
      runningSubagents: {
        [SESSION_ID]: [
          {
            threadId: MAIN_THREAD_ID,
            toolUseId: 'toolu_sub_1',
            subagentType: 'general-purpose',
            description: 'Explore the codebase',
            background: false,
          },
        ],
      },
    });

    renderPane(mockThreads, MAIN_THREAD_ID);

    const indicator = await screen.findByTestId('subagent-running-indicator');
    expect(indicator).toHaveTextContent('Explore the codebase');
    // Indented (ml-6) so it lines up with the tool-call cards rather than the
    // top-level prose — the running subagent is itself a tool in flight.
    expect(indicator).toHaveClass('ml-6');
  });

  it('hides the running-subagent indicator on a thread other than the one that launched it', async () => {
    useLiveStore.setState({
      runningSubagents: {
        [SESSION_ID]: [
          {
            threadId: MAIN_THREAD_ID,
            toolUseId: 'toolu_sub_1',
            subagentType: 'general-purpose',
            description: 'Explore the codebase',
            background: false,
          },
        ],
      },
    });

    // View the sub-thread: the subagent belongs to main, so its activity must
    // not bleed into a different thread of the same session.
    renderPane(mockThreads, BRANCH_THREAD_ID);

    // The breadcrumb confirms the sub-thread pane has rendered before asserting
    // the indicator's absence (so the check is not trivially passing pre-mount).
    await screen.findByRole('navigation', { name: 'Breadcrumb' });
    expect(
      screen.queryByTestId('subagent-running-indicator'),
    ).not.toBeInTheDocument();
  });

  it('shows the permission notice with Allow/Deny and the arguments', async () => {
    // The notice is driven by the `PermissionRequest` hook, which fires only when
    // an interactive dialog actually appears, so it is surfaced directly with no
    // debounce.
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'permission',
            requestId: 7,
            toolName: 'Bash',
            toolInput: '{"command":"rm -rf scratch"}',
            dismissed: false,
            queued: [],
            pendingCount: 1,
          },
        ],
      },
    });

    renderPane();

    const notice = await screen.findByTestId('permission-notice');
    expect(notice).toHaveTextContent('Permission requested: Bash');
    // The arguments show WHAT the tool wants to do, not raw JSON.
    expect(notice).toHaveTextContent('rm -rf scratch');
    expect(within(notice).getByRole('button', { name: 'Allow' })).toBeEnabled();
    expect(within(notice).getByRole('button', { name: 'Deny' })).toBeEnabled();
  });

  it('POSTs the decision on Allow and waits for the resolution event', async () => {
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'permission',
            requestId: 7,
            toolName: 'Bash',
            toolInput: '{"command":"rm -rf scratch"}',
            dismissed: false,
            queued: [],
            pendingCount: 1,
          },
        ],
      },
    });
    const decisions: { id: string; body: unknown }[] = [];
    server.use(
      http.post('*/api/permissions/:id/decision', async ({ params, request }) => {
        decisions.push({ id: String(params.id), body: await request.json() });
        return new HttpResponse(null, { status: 204 });
      }),
    );

    renderPane();
    const notice = await screen.findByTestId('permission-notice');
    fireEvent.click(within(notice).getByRole('button', { name: 'Allow' }));

    await waitFor(() =>
      expect(decisions).toEqual([{ id: '7', body: { decision: 'allow' } }]),
    );
    // The notice itself is cleared by the broadcast `permission_resolved`,
    // exactly like a TUI-answered prompt — not by the POST response.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'permission_resolved',
        session_id: SESSION_ID,
        request_id: 7,
      });
    });
    expect(screen.queryByTestId('permission-notice')).not.toBeInTheDocument();
  });

  it('falls back to the terminal guidance when the decision is a conflict', async () => {
    // 409 permission_not_pending: the hook wait timed out, so the TUI prompt
    // owns the question now. The card swaps Allow/Deny for the guidance.
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'permission',
            requestId: 7,
            toolName: 'Bash',
            toolInput: '{"command":"rm -rf scratch"}',
            dismissed: false,
            queued: [],
            pendingCount: 1,
          },
        ],
      },
    });
    server.use(
      http.post('*/api/permissions/:id/decision', () =>
        HttpResponse.json(
          { error: 'not pending', code: 'permission_not_pending' },
          { status: 409 },
        ),
      ),
    );

    renderPane();
    const notice = await screen.findByTestId('permission-notice');
    fireEvent.click(within(notice).getByRole('button', { name: 'Deny' }));

    expect(
      await within(notice).findByText('Answer the prompt in the terminal.'),
    ).toBeInTheDocument();
    expect(
      within(notice).queryByRole('button', { name: 'Allow' }),
    ).not.toBeInTheDocument();
    expect(
      within(notice).getByRole('button', { name: 'Open terminal' }),
    ).toBeInTheDocument();
  });

  it('clears the permission notice when the request resolves', async () => {
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'permission',
            requestId: 7,
            toolName: 'Bash',
            toolInput: '{"command":"ls"}',
            dismissed: false,
            queued: [],
            pendingCount: 1,
          },
        ],
      },
    });

    renderPane();
    expect(await screen.findByTestId('permission-notice')).toBeInTheDocument();

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'permission_resolved',
        session_id: SESSION_ID,
        request_id: 7,
      });
    });

    expect(screen.queryByTestId('permission-notice')).not.toBeInTheDocument();
  });

  it('shows the queue head with the remaining count and walks it as answers land', async () => {
    // A parallel tool-call fan-out: three approvals outstanding at once. The card
    // shows the OLDEST (the head) and says how many more are waiting, and each
    // resolution promotes the next — so answering walks the queue front to back
    // instead of leaving the rest invisible.
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'permission',
            requestId: 11,
            toolName: 'cat a',
            toolInput: '{"command":"cat a"}',
            dismissed: false,
            queued: [
              {
                requestId: 12,
                toolName: 'cat b',
                toolInput: '{"command":"cat b"}',
              },
              {
                requestId: 13,
                toolName: 'cat c',
                toolInput: '{"command":"cat c"}',
              },
            ],
            pendingCount: 3,
          },
        ],
      },
    });

    renderPane();
    const notice = await screen.findByTestId('permission-notice');
    expect(notice).toHaveTextContent('Permission requested: cat a');
    expect(
      within(notice).getByTestId('permission-notice-remaining'),
    ).toHaveTextContent('+2 more');

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'permission_resolved',
        session_id: SESSION_ID,
        request_id: 11,
      });
    });

    // The next request took the card over — the browser is never left without a
    // dialog while approvals are still pending.
    const promoted = await screen.findByTestId('permission-notice');
    expect(promoted).toHaveTextContent('Permission requested: cat b');
    expect(
      within(promoted).getByTestId('permission-notice-remaining'),
    ).toHaveTextContent('+1 more');

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'permission_resolved',
        session_id: SESSION_ID,
        request_id: 12,
      });
    });
    const last = await screen.findByTestId('permission-notice');
    expect(last).toHaveTextContent('Permission requested: cat c');
    expect(
      within(last).queryByTestId('permission-notice-remaining'),
    ).not.toBeInTheDocument();

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'permission_resolved',
        session_id: SESSION_ID,
        request_id: 13,
      });
    });
    expect(screen.queryByTestId('permission-notice')).not.toBeInTheDocument();
  });

  it('hides the permission card on Dismiss without dropping the notice entry', async () => {
    useLiveStore.setState({
      notices: {
        [SESSION_ID]: [
          {
            kind: 'permission',
            requestId: 7,
            toolName: 'Bash',
            toolInput: '{"command":"ls"}',
            dismissed: false,
            queued: [],
            pendingCount: 1,
          },
        ],
      },
    });

    renderPane();
    const notice = await screen.findByTestId('permission-notice');
    fireEvent.click(within(notice).getByRole('button', { name: 'Dismiss' }));

    // The card goes away, but the entry stays (flagged): the request is still
    // pending server-side, and removal would let the next sends refetch
    // re-seed it and resurrect the card the user just closed.
    expect(screen.queryByTestId('permission-notice')).not.toBeInTheDocument();
    expect(
      noticeOf(useLiveStore.getState().notices, SESSION_ID, 'permission'),
    ).toMatchObject({ requestId: 7, dismissed: true });
  });
});
