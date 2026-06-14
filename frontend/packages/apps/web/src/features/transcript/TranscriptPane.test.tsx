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
import type { MessagesResponse } from '@delta/wire-gen';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  SESSION_ID,
  createHandlers,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { findAllQuoteRanges } from './branchHighlight';
import { TranscriptPane } from './TranscriptPane';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPane(threads = mockThreads, activeThreadId = MAIN_THREAD_ID) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const active = threads.find((t) => t.id === activeThreadId)!;
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane
          threads={threads}
          activeThread={active}
          readOnly={false}
        />
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
      sending: [],
      localSends: {},
      spawns: [],
      notices: {},
      streamingMessages: {},
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
    // Viewing main hides the breadcrumb even though the session has branched
    // (mockThreads contains a sub-thread): a lone "main" crumb is just noise.
    expect(
      screen.queryByRole('navigation', { name: 'Breadcrumb' }),
    ).not.toBeInTheDocument();
  });

  it('shows the caret only while streaming, and keeps the bubble after turn end (suppression owns removal)', async () => {
    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    // A turn streams into the active thread's session: the provisional bubble
    // appears at the tail with the accumulated text, and — while in progress
    // (not final) — the blinking "generating" caret is shown.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: MAIN_THREAD_ID,
        message_id: 'm1',
        index: 0,
        final: false,
        delta: 'streaming reply…',
      });
    });
    const bubble = screen.getByTestId('streaming-message');
    expect(bubble).toHaveTextContent('streaming reply…');
    expect(bubble).toHaveTextContent('▌');

    // The final chunk arrives: the stream is done, so the caret disappears —
    // a completed bubble awaiting handoff must not show a "generating" caret.
    // The bubble itself stays (no persisted copy yet).
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: MAIN_THREAD_ID,
        message_id: 'm1',
        index: 1,
        final: true,
        delta: ' done',
      });
    });
    expect(screen.getByTestId('streaming-message')).toHaveTextContent(
      'streaming reply… done',
    );
    expect(screen.getByTestId('streaming-message')).not.toHaveTextContent('▌');

    // turn_completed no longer drops the buffer: without a matching persisted
    // message, the (caret-less) bubble lingers rather than leaving a gap. Its
    // removal is owned by the suppression guard once the persisted copy lands.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'turn_completed',
        session_id: SESSION_ID,
        stop_reason: null,
      });
    });
    expect(screen.getByTestId('streaming-message')).toBeInTheDocument();
  });

  it('renders the live streaming bubble as Markdown', async () => {
    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    // A streamed delta carrying Markdown renders through AssistantMarkdown, the
    // same component the persisted message uses, so `**bold**` becomes a real
    // <strong> inside the provisional bubble — not raw asterisks.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: MAIN_THREAD_ID,
        message_id: 'm1',
        index: 0,
        final: false,
        delta: 'hello **bold**',
      });
    });
    const bubble = screen.getByTestId('streaming-message');
    const strong = within(bubble).getByText('bold');
    expect(strong.tagName).toBe('STRONG');
  });

  it('hides the live bubble once its text is persisted, even before turn end', async () => {
    // The handoff bug: the transcript refetch can persist the assistant reply
    // BEFORE the turn-end event clears the streaming buffer, so for a moment
    // both the live bubble and the persisted message-item carry the same text.
    // The content-based guard suppresses the bubble the instant a matching
    // persisted assistant message exists, regardless of event/refetch ordering.
    const reply = 'A **delta** is the change between two states.';
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
              content_text: 'What is a delta?',
              content: [{ type: 'text', text: 'What is a delta?' }],
              created_at: '2026-01-01T00:00:01Z',
            },
            {
              uuid: 'm-assistant',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-user',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: reply,
              content: [{ type: 'text', text: reply }],
              created_at: '2026-01-01T00:00:02Z',
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane(mockThreads, MAIN_THREAD_ID);
    // The persisted assistant reply renders via the normal pipeline.
    await waitFor(() =>
      expect(screen.getByText(/change between two states/)).toBeInTheDocument(),
    );

    // The streaming buffer still holds the same text (the turn-end event has
    // not landed yet). With the persisted copy already present, the live bubble
    // must NOT render — the text appears exactly once.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: MAIN_THREAD_ID,
        message_id: 'm1',
        index: 0,
        final: true,
        delta: reply,
      });
    });
    expect(screen.queryByTestId('streaming-message')).not.toBeInTheDocument();
    // The reply's distinctive text appears exactly once (the persisted item).
    expect(screen.getAllByText(/change between two states/)).toHaveLength(1);
  });

  it('hides the live bubble in a tool turn where the persisted text is followed by a tool_use message', async () => {
    // The tool-turn handoff bug: Claude splits a single assistant reply into
    // separate per-content-block transcript lines, so the visible text lives in
    // one assistant message while a LATER assistant message carries only a
    // tool_use block (empty visible text). The content guard must scan ALL
    // assistant messages — not just the last — so the bubble is suppressed and
    // the streamed text appears exactly once.
    const reply = 'A **delta** is the change between two states.';
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
              content_text: 'What is a delta?',
              content: [{ type: 'text', text: 'What is a delta?' }],
              created_at: '2026-01-01T00:00:01Z',
            },
            {
              uuid: 'm-assistant-text',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-user',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: reply,
              content: [{ type: 'text', text: reply }],
              created_at: '2026-01-01T00:00:02Z',
            },
            {
              // The tool_use block of the SAME reply, persisted as its own line
              // with no visible text — and it is the last assistant message.
              uuid: 'm-assistant-tool',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-assistant-text',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 2,
              content_text: '',
              content: [
                { type: 'tool_use', id: 't1', name: 'Bash', input: { command: 'ls' } },
              ],
              created_at: '2026-01-01T00:00:03Z',
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText(/change between two states/)).toBeInTheDocument(),
    );

    // The streaming buffer still holds the reply text (turn not ended yet). With
    // the text persisted on an earlier line and a tool_use line last, the live
    // bubble must NOT render — the text appears exactly once.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: MAIN_THREAD_ID,
        message_id: 'm1',
        index: 0,
        final: true,
        delta: reply,
      });
    });
    expect(screen.queryByTestId('streaming-message')).not.toBeInTheDocument();
    expect(screen.getAllByText(/change between two states/)).toHaveLength(1);
  });

  it('keeps showing the live bubble when a partial stream shares a prefix with an earlier reply', async () => {
    // False-positive guard: the previous turn's persisted assistant reply opens
    // the same way the new reply is starting (a common "Let me…" opener). The
    // growing partial stream must NOT be suppressed by that earlier message —
    // `startsWith` is gated on a final stream, so a non-final partial prefix
    // never matches, and the new reply is not persisted yet, so the live bubble
    // must still render.
    const earlierReply = 'Let me check that for you. Answer one.';
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
              content_text: 'first question',
              content: [{ type: 'text', text: 'first question' }],
              created_at: '2026-01-01T00:00:01Z',
            },
            {
              uuid: 'm-assistant',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-user',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: earlierReply,
              content: [{ type: 'text', text: earlierReply }],
              created_at: '2026-01-01T00:00:02Z',
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText(/Answer one\./)).toBeInTheDocument(),
    );

    // A new reply streams in, so far only "Let me check" — a prefix of the
    // persisted earlier reply. It is not final and not yet persisted, so the
    // bubble must show.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: MAIN_THREAD_ID,
        message_id: 'm2',
        index: 0,
        final: false,
        delta: 'Let me check',
      });
    });
    expect(screen.getByTestId('streaming-message')).toHaveTextContent(
      'Let me check',
    );
  });

  it('does not render the live bubble for a different thread', async () => {
    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    // A preview attributed to another thread of the same session must not show
    // on the thread the user is viewing.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'assistant_streaming',
        session_id: SESSION_ID,
        thread_id: BRANCH_THREAD_ID,
        message_id: 'm1',
        index: 0,
        final: false,
        delta: 'on a branch',
      });
    });
    expect(screen.queryByTestId('streaming-message')).not.toBeInTheDocument();
  });

  it('shows the breadcrumb with "main" as an ancestor while viewing a sub-thread', async () => {
    // Drilled into a sub-thread (ancestry = [main › delta etymology]), so the
    // breadcrumb appears with "main" as a clickable leading crumb.
    renderPane(mockThreads, BRANCH_THREAD_ID);

    const breadcrumb = await screen.findByRole('navigation', {
      name: 'Breadcrumb',
    });
    expect(
      within(breadcrumb).getByRole('button', { name: 'main' }),
    ).toBeInTheDocument();
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

  it('scrolls the origin chip into view when going up via the breadcrumb', async () => {
    // jsdom does not implement scrollIntoView; spy on it for the assertion.
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;

    const main = mockThreads.find((t) => t.id === MAIN_THREAD_ID)!;
    const branch = mockThreads.find((t) => t.id === BRANCH_THREAD_ID)!;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const client = new ApiClient({ baseUrl: 'http://localhost' });
    const ui = (active: typeof main) => (
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={client}>
          <TranscriptPane threads={mockThreads} activeThread={active} readOnly={false} />
        </ApiProvider>
      </QueryClientProvider>
    );

    // Start drilled into the sub-thread, then click "main" in the breadcrumb.
    const { rerender } = render(ui(branch));
    fireEvent.click(await screen.findByRole('button', { name: 'main' }));

    // The workspace reconciles the active thread to main after the click.
    rerender(ui(main));

    // Once main (and the branch's origin chip) render, that chip — not the
    // bottom of the parent — is scrolled into view.
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    const target = scrollIntoView.mock.instances[0] as HTMLElement;
    expect(target.getAttribute('data-child-thread-id')).toBe(
      String(BRANCH_THREAD_ID),
    );
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
            {
              uuid: 'm-meta',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'meta',
              linear_parent_uuid: 'm-other',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 3,
              content_text: 'INJECTED META BODY',
              content: [{ type: 'text', text: 'INJECTED META BODY' }],
              created_at: '2026-01-01T00:00:04Z',
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
    // Meta lines are rendered (unlike system/other), but collapsed: the summary
    // shows a `meta` badge plus the first line, and the disclosure starts closed.
    expect(screen.getByText('meta')).toBeInTheDocument();
    expect(screen.getByText('INJECTED META BODY')).toBeInTheDocument();
    const metaItem = screen
      .getByText('INJECTED META BODY')
      .closest('[data-testid="message-item"]');
    expect(metaItem).toHaveAttribute('data-role', 'meta');
    expect(metaItem?.querySelector('button')).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  });

  it('left-indents the task-notification card like a tool row, but not ordinary user prose', async () => {
    // The harness-injected task-notification card is a nested aside (like a
    // tool-execution row), so its block wrapper carries the same `ml-6` left
    // indent. An ordinary user prose turn must stay at full width (no indent).
    server.use(
      http.get('*/api/threads/:id/messages', () => {
        const body: MessagesResponse = {
          messages: [
            {
              uuid: 'm-user-prose',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'user',
              linear_parent_uuid: null,
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 0,
              content_text: 'ordinary prose turn',
              content: [{ type: 'text', text: 'ordinary prose turn' }],
              created_at: '2026-01-01T00:00:01Z',
            },
            {
              uuid: 'm-task-notification',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'user',
              linear_parent_uuid: 'm-user-prose',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: '<task-notification>background job done',
              content: [
                { type: 'text', text: '<task-notification>background job done' },
              ],
              created_at: '2026-01-01T00:00:02Z',
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane();

    // The task-notification card renders folded; its message-item article carries
    // the data-task-notification marker. Its block wrapper (the parent div) owns
    // the gap/indent decision and must be left-indented like a tool row.
    const notificationItem = await waitFor(() => {
      const item = document.querySelector(
        '[data-task-notification="true"]',
      );
      expect(item).not.toBeNull();
      return item!;
    });
    const notificationBlock = notificationItem.parentElement;
    expect(notificationBlock?.className).toContain('ml-6');

    // The ordinary user prose turn is NOT indented (regression guard).
    const proseBlock = screen
      .getByText('ordinary prose turn')
      .closest('[data-testid="message-item"]')?.parentElement;
    expect(proseBlock?.className).not.toContain('ml-6');
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

  it('shows the permission notice with Allow/Deny and the input summary', async () => {
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
          },
        ],
      },
    });

    renderPane();

    const notice = await screen.findByTestId('permission-notice');
    expect(notice).toHaveTextContent('Permission requested: Bash');
    // The input summary shows WHAT the tool wants to do, not raw JSON.
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

  it('clears a pending branch selection on a plain (collapsed) click in the transcript body', async () => {
    // A passage was selected for "Branch from selected text" (a pending
    // branchOrigin on the active thread). A plain click in the conversation —
    // one that leaves the selection collapsed — drops it, so dismissing no
    // longer requires the composer's ✕.
    useComposerStore.setState({
      branchOrigin: {
        parentThreadId: MAIN_THREAD_ID,
        semanticParentUuid: 'm-user',
        locatorQuote: 'selected passage',
      },
    });
    // A plain click collapses the selection.
    const getSelection = vi
      .spyOn(window, 'getSelection')
      .mockReturnValue({ isCollapsed: true } as Selection);

    renderPane();
    const message = await screen.findByText('What is a delta?');

    fireEvent.click(message);

    await waitFor(() =>
      expect(useComposerStore.getState().branchOrigin).toBeNull(),
    );
    getSelection.mockRestore();
  });

  it('keeps a pending branch selection when a click leaves a non-empty selection (drag-select end)', async () => {
    // The mouseup that finishes a drag-select also fires a click, but it leaves
    // a non-empty (non-collapsed) selection — the one that just set the branch
    // origin. That click must NOT immediately undo it.
    const origin = {
      parentThreadId: MAIN_THREAD_ID,
      semanticParentUuid: 'm-user' as const,
      locatorQuote: 'selected passage',
    };
    useComposerStore.setState({ branchOrigin: origin });
    const getSelection = vi
      .spyOn(window, 'getSelection')
      .mockReturnValue({ isCollapsed: false } as Selection);

    renderPane();
    const message = await screen.findByText('What is a delta?');

    fireEvent.click(message);

    // The branch origin survives a non-collapsed click.
    expect(useComposerStore.getState().branchOrigin).toEqual(origin);
    getSelection.mockRestore();
  });

  it('paints the pending branch quote in the body via the branch-origin highlight', async () => {
    // While a branch is pending, its selected passage stays highlighted (the
    // CSS Custom Highlight API), so it is visible even after focus moves to the
    // composer textarea and the native selection fades. The effect searches the
    // rendered message bodies for the branchOrigin quote; in jsdom the highlight
    // registry may be unavailable, so this asserts the guarded, no-throw path
    // and that the range computation runs against the quote.
    useComposerStore.setState({
      branchOrigin: {
        parentThreadId: MAIN_THREAD_ID,
        semanticParentUuid: 'm-user',
        locatorQuote: 'What is a delta?',
      },
    });

    expect(() => renderPane()).not.toThrow();
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    // The highlighted passage occurs verbatim in a rendered message body, so the
    // range computation finds at least one match (proving the effect targeted
    // the branchOrigin quote, independent of whether jsdom paints it).
    const body = screen
      .getByText('What is a delta?')
      .closest('[data-testid="message-item"]')!;
    expect(findAllQuoteRanges(body, 'What is a delta?').length).toBeGreaterThan(
      0,
    );
  });

  describe('dynamic bottom reserve (composer auto-grow follow)', () => {
    // The body reserves bottom space equal to the bottom overlay's MEASURED
    // height, so the composer growing pushes the conversation tail up instead of
    // hiding it. jsdom performs no layout (every `getBoundingClientRect` is 0 and
    // ResizeObserver never fires on its own), so we drive both explicitly: stub
    // the overlay's measured height and a controllable ResizeObserver, then fire
    // it to simulate the composer growing.

    /** The single live observer instance and the element it watches. */
    let observed: { el: Element; cb: ResizeObserverCallback } | null;
    let originalRO: typeof ResizeObserver;

    beforeEach(() => {
      observed = null;
      originalRO = globalThis.ResizeObserver;
      class ControllableRO implements ResizeObserver {
        constructor(private cb: ResizeObserverCallback) {}
        observe(el: Element): void {
          observed = { el, cb: this.cb };
        }
        unobserve(): void {}
        disconnect(): void {
          observed = null;
        }
      }
      globalThis.ResizeObserver =
        ControllableRO as unknown as typeof ResizeObserver;
    });

    afterEach(() => {
      globalThis.ResizeObserver = originalRO;
    });

    /** The Panel scroll body (the element that carries the reserve padding). */
    function bodyEl(): HTMLElement {
      return document.querySelector('.scrollbar-hover') as HTMLElement;
    }

    it('creates a ResizeObserver for the bottom overlay and drives padding-bottom from its measured height', async () => {
      renderPane();
      await waitFor(() =>
        expect(screen.getByTestId('bottom-overlay')).toBeInTheDocument(),
      );
      const overlay = screen.getByTestId('bottom-overlay');

      // The overlay-measuring observer is watching the overlay node itself.
      await waitFor(() => expect(observed?.el).toBe(overlay));

      // Stub the overlay's measured height, then fire the observer as a real
      // resize would. The body's padding-bottom = measured height + the overlay
      // inset gap (12px fallback in jsdom, which computes no custom-property) +
      // the 64px reading gap that keeps the last turn off the composer.
      overlay.getBoundingClientRect = () =>
        ({ height: 120 }) as DOMRect;
      act(() => observed!.cb([], observed!.cb as unknown as ResizeObserver));

      await waitFor(() =>
        expect(bodyEl().style.paddingBottom).toBe('196px'),
      );
    });

    it('grows the reserve when the overlay grows (composer auto-grow), keeping the tail above it', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(observed?.el).toBe(overlay));

      overlay.getBoundingClientRect = () => ({ height: 80 }) as DOMRect;
      act(() => observed!.cb([], observed!.cb as unknown as ResizeObserver));
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('156px'));

      // The composer grows (more lines typed): the overlay is taller, so the
      // reserve grows in lockstep — the last turn stays clear of the input.
      overlay.getBoundingClientRect = () => ({ height: 200 }) as DOMRect;
      act(() => observed!.cb([], observed!.cb as unknown as ResizeObserver));
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('276px'));
    });

    it('re-sticks the body to the bottom when the overlay grows while sticking', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(observed?.el).toBe(overlay));

      // Make the body look scrollable and pinned at the bottom (sticking). jsdom
      // reports 0 for layout, so define the scroll geometry by hand.
      const body = bodyEl();
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      // Start pinned to the bottom so stickRef stays true.
      body.scrollTop = 600;
      fireEvent.scroll(body);

      overlay.getBoundingClientRect = () => ({ height: 150 }) as DOMRect;
      act(() => observed!.cb([], observed!.cb as unknown as ResizeObserver));

      // The reserve grew (overlay 150 + 12 inset); the measurement re-stuck the
      // body to the new bottom (scrollTop := scrollHeight) so the tail stays
      // visible just above the grown composer.
      await waitFor(() =>
        expect(body.style.paddingBottom).toBe('226px'),
      );
      expect(body.scrollTop).toBe(1000);
    });

    it('does not move the body when the user has scrolled up (not sticking)', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(observed?.el).toBe(overlay));

      const body = bodyEl();
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      // Scrolled well up: far from the bottom, so stickRef goes false.
      body.scrollTop = 100;
      fireEvent.scroll(body);

      overlay.getBoundingClientRect = () => ({ height: 150 }) as DOMRect;
      act(() => observed!.cb([], observed!.cb as unknown as ResizeObserver));

      // Reading scrollback is not yanked to the bottom; only the reserve updates.
      await waitFor(() =>
        expect(body.style.paddingBottom).toBe('226px'),
      );
      expect(body.scrollTop).toBe(100);
    });
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
