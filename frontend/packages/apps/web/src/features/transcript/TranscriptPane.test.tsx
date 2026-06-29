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
import {
  TIMELINE_EXPANDED_SUBKEY,
  resetTimelineExpandedForTests,
} from './ThreadTimelineOverlay';
import { sessionScopedKey } from '../../store/sessionScopedStorage';

/**
 * Compose the localStorage key the timeline uses for the mock session every
 * test in this file points at. The TranscriptPane reads the focused session
 * id from `navStore`; cases that seed an expanded preference (or assert one)
 * also pin the focus to {@link SESSION_ID} so the hook persists against the
 * matching per-session key.
 */
function timelineExpandedKey(sessionId: string = SESSION_ID): string {
  return sessionScopedKey(sessionId, TIMELINE_EXPANDED_SUBKEY);
}
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
      runningSubagents: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
    // Clear the timeline-expanded persisted preference and the in-memory
    // cache so each test starts from the default collapsed state (some
    // tests opt into expanded explicitly). The cache resets every per-session
    // entry when called without an id; clearing localStorage in lockstep
    // wipes any session-scoped keys an earlier case may have written.
    window.localStorage.clear();
    resetTimelineExpandedForTests();
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

  it('shows the question card only on the thread the question was asked on', async () => {
    // The question is asked on the main thread; viewing main, the card shows.
    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'question_asked',
        session_id: SESSION_ID,
        request_id: 5,
        thread_id: MAIN_THREAD_ID,
        tool_input: '{"questions":[{"header":"Pick"}]}',
      });
    });
    expect(await screen.findByTestId('question-card')).toBeInTheDocument();
  });

  it('does not render the question card for a different thread', async () => {
    renderPane(mockThreads, MAIN_THREAD_ID);
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    // A question attributed to another thread of the same session must not
    // show on the thread the user is viewing — only on its own thread.
    act(() => {
      useLiveStore.getState().applyEvent({
        kind: 'question_asked',
        session_id: SESSION_ID,
        request_id: 5,
        thread_id: BRANCH_THREAD_ID,
        tool_input: '{"questions":[{"header":"Pick"}]}',
      });
    });
    expect(screen.queryByTestId('question-card')).not.toBeInTheDocument();
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
    // A meta line is a nested aside (like a tool row / task-notification card),
    // so its block wrapper carries the same `ml-6` left indent.
    expect(metaItem?.parentElement?.className).toContain('ml-6');
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

  it('lands on the Repository tab in the new-session state', async () => {
    // Phase B retired the auto-opened modal: the new-session screen shows
    // the 3-tab picker (PR / Repository / Directory) inline and defaults to
    // Repository, which is the recency-ordered registered-repo list.
    renderNewSessionPane();

    expect(
      await screen.findByTestId('new-session-tab-repository'),
    ).toHaveAttribute('aria-selected', 'true');
    // The modal is NOT auto-opened anymore.
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('hoists the tab strip into the Panel header instead of a "New session" label', async () => {
    // The tabs are pinned at the top of the pane (the Panel header lives
    // outside the scrolling body), and the plain "New session" label is
    // gone — the tabs convey the screen's identity.
    renderNewSessionPane();
    const tablist = await screen.findByRole('tablist', {
      name: 'Start a session from',
    });
    // The Panel renders its header inside a <header> element; the tablist
    // sits inside it, above the scroll viewport.
    expect(tablist.closest('header')).not.toBeNull();
    // The old plain "New session" label is removed.
    expect(screen.queryByText('New session', { exact: true })).toBeNull();
  });

  it('does not pop a modal even when workdirMandatory is set', async () => {
    // First run (no sessions to fall back to): in Phase B the tabbed picker
    // replaces the auto-opened modal entirely, so `workdirMandatory` has no
    // modal to gate. The new-session intent is preserved, and the Directory
    // tab is reachable for the inline picker. The modal's own
    // non-dismissable behaviour is exercised by WorkdirDialog.test.tsx.
    renderNewSessionPane(mockThreads, { workdirMandatory: true });

    expect(
      await screen.findByTestId('new-session-tab-directory'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
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

  it('shows no chip and disables Send when no workdir is selected (no session to return to)', async () => {
    // Phase B: no auto-opened modal. The new-session screen waits for the
    // user to pick a starting point from the inline tabs. With nothing
    // picked, no chip shows and Send stays disabled; the new-session intent
    // is preserved because the empty initial screen has nowhere to return
    // to.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: null,
    });
    renderNewSessionPane();

    expect(await screen.findByTestId('new-session-tabs')).toBeInTheDocument();
    expect(screen.queryByTestId('workdir-chip')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('returns to the previously-focused session when the chip-opened modal is dismissed without a selection', async () => {
    // Phase B: there is no auto-opened modal. The cancel path here is
    // chip ✎ → modal → Cancel. To exercise it the test seeds a workdir
    // (so the chip is present), clicks the chip's pencil button to open
    // the picker, then cancels. With nothing in `newSessionWorkdir` after
    // the (no-op) cancel, the empty-back-out path restores the previously-
    // focused session. Strictly: the cancel clears the candidate via the
    // dialog's onClose, but does NOT clear the already-committed selection,
    // so the contract here is "previously-focused session restoration
    // only fires when the workdir is unset at dismiss time". To mirror
    // that, the test clears the workdir right before clicking Cancel.
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: SESSION_ID,
    });
    useComposerStore.setState({ newSessionWorkdir: '/home/dev/projects/delta' });
    renderNewSessionPane();

    fireEvent.click(
      within(screen.getByTestId('workdir-chip')).getByRole('button', {
        name: 'Change working directory',
      }),
    );

    // Clear the seeded workdir so cancellation takes the
    // "no selection at dismiss time" path that restores the prior focus.
    useComposerStore.setState({ newSessionWorkdir: null });
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

    /**
     * Live observations keyed by the observed element. TranscriptPane creates
     * several ResizeObservers (body re-stick, bottom overlay, top region), so
     * the per-test code looks up exactly the observer it cares about by node
     * rather than reading the "most recent" one, which would race with effect
     * ordering.
     */
    let observations: Map<Element, ResizeObserverCallback>;
    let originalRO: typeof ResizeObserver;

    function lookupObserver(el: Element | null): ResizeObserverCallback | null {
      return el ? observations.get(el) ?? null : null;
    }

    beforeEach(() => {
      observations = new Map();
      originalRO = globalThis.ResizeObserver;
      class ControllableRO implements ResizeObserver {
        private observedEls = new Set<Element>();
        constructor(private cb: ResizeObserverCallback) {}
        observe(el: Element): void {
          observations.set(el, this.cb);
          this.observedEls.add(el);
        }
        unobserve(el: Element): void {
          observations.delete(el);
          this.observedEls.delete(el);
        }
        disconnect(): void {
          for (const el of this.observedEls) {
            observations.delete(el);
          }
          this.observedEls.clear();
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
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());
      const fire = lookupObserver(overlay)!;

      // Stub the overlay's measured height, then fire the observer as a real
      // resize would. The body's padding-bottom = measured height + the overlay
      // inset gap (12px fallback in jsdom, which computes no custom-property) +
      // the 192px reading gap that keeps the last turn off the composer.
      overlay.getBoundingClientRect = () =>
        ({ height: 120 }) as DOMRect;
      act(() => fire([], fire as unknown as ResizeObserver));

      await waitFor(() =>
        expect(bodyEl().style.paddingBottom).toBe('324px'),
      );
    });

    it('grows the reserve when the overlay grows (composer auto-grow), keeping the tail above it', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

      overlay.getBoundingClientRect = () => ({ height: 80 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('284px'));

      // The composer grows (more lines typed): the overlay is taller, so the
      // reserve grows in lockstep — the last turn stays clear of the input.
      overlay.getBoundingClientRect = () => ({ height: 200 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('404px'));
    });

    it('re-sticks the body to the bottom when the overlay grows while sticking', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

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
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });

      // The reserve grew (overlay 150 + 12 inset); the measurement re-stuck the
      // body to the new bottom (scrollTop := scrollHeight) so the tail stays
      // visible just above the grown composer.
      await waitFor(() =>
        expect(body.style.paddingBottom).toBe('354px'),
      );
      expect(body.scrollTop).toBe(1000);
    });

    it('does not move the body when the user has scrolled up (not sticking)', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

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
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });

      // Reading scrollback is not yanked to the bottom; only the reserve updates.
      await waitFor(() =>
        expect(body.style.paddingBottom).toBe('354px'),
      );
      expect(body.scrollTop).toBe(100);
    });

    it('holds the reserve and skips the re-scroll while a branch is being composed', async () => {
      // Selecting text for "Branch from selected text" sets branchOrigin and
      // makes the overlay taller (the banner renders inside it). Folding that
      // into the reserve and re-scrolling would shift the transcript the instant
      // text is selected — moving the very selection the user is adjusting. So
      // while a branch is pending, the overlay's resize is ignored: the reserve
      // stays put and the body is not re-stuck to the bottom.
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

      // Establish a baseline reserve before any branch is pending.
      overlay.getBoundingClientRect = () => ({ height: 80 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('284px'));

      // Pin the body to the bottom (sticking) so a re-scroll would be visible.
      const body = bodyEl();
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      body.scrollTop = 600;
      fireEvent.scroll(body);

      // A branch is now pending: the banner grows the overlay.
      act(() =>
        useComposerStore.setState({
          branchOrigin: {
            parentThreadId: MAIN_THREAD_ID,
            semanticParentUuid: 'm-user',
            locatorQuote: 'What is a delta?',
          },
        }),
      );
      overlay.getBoundingClientRect = () => ({ height: 200 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });

      // The grown banner does NOT grow the reserve and does NOT re-stick the
      // body: the banner floats over the transcript tail instead of pushing it.
      expect(body.style.paddingBottom).toBe('284px');
      expect(body.scrollTop).toBe(600);
    });

    // v19 top-row layout. The collapsed state places the breadcrumb and
    // the {Thread + Terminal} cluster as INDIVIDUAL absolute floating
    // cards over the body (preserved from v18). The expanded state
    // wraps the entire top region in a SINGLE absolute container
    // pinned to the top of the Panel body region — it does NOT scroll
    // with the conversation, so scrubbing the timeline (which scrolls
    // the conversation) no longer drags the timeline off-screen the
    // way v18's in-flow expanded layout did. Inside the container the
    // children use normal flow: the timeline card on top, the
    // breadcrumb + Terminal row underneath. The body reserves
    // `padding-top` in BOTH states (via `--delta-top-region-reserve`)
    // so the first message clears the pinned region.
    describe('top region overlay (v19 pinned expanded container)', () => {
      it('renders the breadcrumb and the right-side cluster as individual absolute cards in the collapsed state — no shared white bar wrapper', async () => {
        renderPane(mockThreads, BRANCH_THREAD_ID);
        const topRegion = await screen.findByTestId('transcript-top-region');
        // The collapsed wrapper itself is layout-less (`display: contents`)
        // — there is no full-width white bar with a background, padding,
        // and stretched left/right insets the v17 layout used. A
        // regression that re-introduced that bar would put `bg-surface`
        // back on the wrapper or some ancestor.
        expect(topRegion.getAttribute('data-expanded')).toBe('false');
        // Negative lookahead excludes `bg-surface-elevated` (a different
        // semantic token used elsewhere) from the substring match.
        expect(topRegion.className).not.toMatch(/\bbg-surface(?!-)/);
        expect(topRegion.className).not.toContain('left-0');
        expect(topRegion.className).not.toContain('right-0');

        // Each floating card is itself an absolute element, pinned with
        // the shared `overlay-inset` token so they read as one row.
        const breadcrumb = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        expect(breadcrumb.className).toContain('absolute');
        expect(breadcrumb.className).toContain('top-overlay-inset');
        expect(breadcrumb.className).toContain('left-overlay-inset');
        expect(breadcrumb.className).toContain('z-20');

        const rightCluster = await screen.findByTestId('transcript-top-row');
        expect(rightCluster.getAttribute('data-expanded')).toBe('false');
        expect(rightCluster.className).toContain('absolute');
        expect(rightCluster.className).toContain('top-overlay-inset');
        expect(rightCluster.className).toContain('right-overlay-inset');
        expect(rightCluster.className).toContain('z-20');
        // The right cluster carries NO shared surface-bar background or
        // border on purpose: each pill inside already has its own card
        // chrome (TIMELINE_TOGGLE_BUTTON_CLASS / TERMINAL_TOGGLE_BUTTON_CLASS).
        // A regression that re-introduced the v17 single-bar look would
        // put `bg-surface` back on this cluster wrapper.
        // Negative lookahead excludes `bg-surface-elevated` (a different
        // semantic token used elsewhere) from the substring match.
        expect(rightCluster.className).not.toMatch(/\bbg-surface(?!-)/);

        // The breadcrumb and the right cluster are siblings under the
        // top-region wrapper, NOT nested inside one shared white-bar
        // container.
        expect(breadcrumb.parentElement).toBe(rightCluster.parentElement);

        // The breadcrumb and timeline toggle render inside their
        // respective floating cards (so both still live under the
        // `transcript-top-region` umbrella).
        expect(
          within(breadcrumb).getByRole('navigation', { name: 'Breadcrumb' }),
        ).toBeInTheDocument();
        expect(
          within(rightCluster).getByTestId('thread-timeline-toggle'),
        ).toBeInTheDocument();
      });

      it("reserves the body's padding-top from --delta-top-region-reserve so the first message clears the floating cards", async () => {
        renderPane(mockThreads, BRANCH_THREAD_ID);
        // The body reads the variable through `padding-top` (set inline so
        // jsdom can read it back as a literal CSS value). Without this
        // reserve the first message would render under the absolute
        // floating cards on initial paint.
        expect(bodyEl().style.paddingTop).toBe(
          'var(--delta-top-region-reserve, 0)',
        );
      });

      it('places the Terminal button inside the right-side cluster alongside the timeline toggle when collapsed', async () => {
        // The Terminal button is forwarded into TranscriptPane as the
        // `terminalButton` slot (see WorkspaceScreen). It must render
        // INSIDE the floating right-side cluster so it stays on screen
        // alongside the timeline toggle; if it were a sibling outside
        // the cluster, it would scroll away with the conversation.
        const queryClient = new QueryClient({
          defaultOptions: { queries: { retry: false } },
        });
        const client = new ApiClient({ baseUrl: 'http://localhost' });
        render(
          <QueryClientProvider client={queryClient}>
            <ApiProvider client={client}>
              <TranscriptPane
                threads={mockThreads}
                activeThread={mockThreads.find((t) => t.id === MAIN_THREAD_ID)!}
                readOnly={false}
                terminalButton={
                  <button data-testid="terminal-toggle">Terminal</button>
                }
              />
            </ApiProvider>
          </QueryClientProvider>,
        );

        const row = await screen.findByTestId('transcript-top-row');
        expect(row.getAttribute('data-expanded')).toBe('false');
        const toggle = within(row).getByTestId('thread-timeline-toggle');
        const terminal = within(row).getByTestId('terminal-toggle');
        expect(toggle).toBeInTheDocument();
        expect(terminal).toBeInTheDocument();
        // DOM order: timeline toggle first, then the Terminal button —
        // so the two pills read left-to-right inside the right cluster.
        expect(
          toggle.compareDocumentPosition(terminal) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      });

      it('places the breadcrumb and Terminal as a single normal-flow row under the expanded timeline card', async () => {
        // Seed the persisted preference to expanded so the timeline
        // mounts open. The expanded layout drops the absolute overlay
        // entirely: the expanded card grows full-width on top in normal
        // flow, and a single row carrying the breadcrumb (left) and the
        // Terminal button (right) sits directly underneath it. The
        // Thread icon is absent in this row — the expanded card itself
        // replaces it.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        const queryClient = new QueryClient({
          defaultOptions: { queries: { retry: false } },
        });
        const client = new ApiClient({ baseUrl: 'http://localhost' });
        render(
          <QueryClientProvider client={queryClient}>
            <ApiProvider client={client}>
              <TranscriptPane
                threads={mockThreads}
                activeThread={mockThreads.find((t) => t.id === BRANCH_THREAD_ID)!}
                readOnly={false}
                terminalButton={
                  <button data-testid="terminal-toggle">Terminal</button>
                }
              />
            </ApiProvider>
          </QueryClientProvider>,
        );

        const region = await screen.findByTestId('transcript-top-region');
        expect(region.getAttribute('data-expanded')).toBe('true');

        const row = await screen.findByTestId('transcript-top-row');
        expect(row.getAttribute('data-expanded')).toBe('true');
        // Single in-flow row, laid out horizontally with the breadcrumb
        // on the left and the Terminal on the right.
        expect(row.className).toContain('flex');
        expect(row.className).not.toContain('flex-col');
        // Both pieces live inside this same row — no separate breadcrumb
        // row of its own.
        const breadcrumbNav = within(row).getByRole('navigation', {
          name: 'Breadcrumb',
        });
        const terminal = within(row).getByTestId('terminal-toggle');
        expect(breadcrumbNav).toBeInTheDocument();
        expect(terminal).toBeInTheDocument();
        // DOM order: breadcrumb first, then Terminal.
        expect(
          breadcrumbNav.compareDocumentPosition(terminal) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
        // The expanded timeline card lives in the region above this row,
        // not inside it — the row is the one underneath.
        expect(
          within(row).queryByTestId('thread-timeline-overlay'),
        ).not.toBeInTheDocument();
        // There is no Thread toggle in the under-row either: the
        // expanded card replaces it in this state.
        expect(
          within(row).queryByTestId('thread-timeline-toggle'),
        ).not.toBeInTheDocument();
        // The expanded timeline card sits ABOVE the under-row inside
        // the top region.
        const expandedCard = within(region).getByTestId(
          'thread-timeline-overlay',
        );
        expect(
          expandedCard.compareDocumentPosition(row) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      });

      it('pins the expanded top region as a SINGLE absolute container so it does not scroll with the conversation (v19 regression fix)', async () => {
        // The v18 expanded layout placed the timeline card + the
        // breadcrumb/Terminal under-row directly into the scrolling
        // body's normal flow. Scrubbing the timeline jumps the
        // conversation; with the timeline in flow, the conversation
        // scroll dragged the timeline itself off-screen and the user
        // could not scrub again. v19 wraps the entire top region in a
        // single absolute container that pins to the top of the Panel
        // body region — the container does not participate in the
        // body's scroll. This test pins that pinning contract.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        renderPane(mockThreads, BRANCH_THREAD_ID);

        const region = await screen.findByTestId('transcript-top-region');
        expect(region.getAttribute('data-expanded')).toBe('true');
        // The container is `absolute top-0 left-0 right-0 z-20` so it
        // anchors to the closest positioned ancestor (the Panel body
        // wrapper, which is `position: relative`) — outside the
        // scrolling body — and stays glued to the top edge across
        // conversation scroll. Without these classes the v18 bug
        // returns: the container would flow with the conversation.
        expect(region.className).toContain('absolute');
        expect(region.className).toContain('top-0');
        expect(region.className).toContain('left-0');
        expect(region.className).toContain('right-0');
        expect(region.className).toContain('z-20');

        // Proxy for "container stays inside the viewport after a body
        // scroll": the container's closest positioned ancestor — the
        // element absolute escapes to — is OUTSIDE the scrolling body
        // (the Panel body div with `overflow-y-auto`). Walking the
        // ancestry from the container, we should reach the relative
        // positioning context BEFORE crossing the scroll body, so the
        // container is anchored to the viewport-pinning region and
        // never to the scroll content. A regression that put the
        // container inside the scrolling body's positioning context
        // (e.g. by making the body itself `position: relative`, or by
        // dropping `absolute`) would fail this check.
        const body = bodyEl();
        let cursor: HTMLElement | null = region.parentElement;
        let positionedAncestor: HTMLElement | null = null;
        while (cursor !== null) {
          // jsdom does not compute layout, but className inspection is
          // enough: the Panel wrapper carries the `relative` Tailwind
          // class, the scroll body carries `overflow-y-auto`.
          if (cursor.className.includes('relative')) {
            positionedAncestor = cursor;
            break;
          }
          cursor = cursor.parentElement;
        }
        expect(positionedAncestor).not.toBeNull();
        // The relative ancestor must be the Panel body region's
        // wrapper, which is the PARENT of the scrolling body — NOT
        // the scrolling body itself or any descendant of it.
        expect(positionedAncestor!.contains(body)).toBe(true);
        expect(body.contains(positionedAncestor!)).toBe(false);
      });

      it('renders the expanded under-row as a normal-flow child INSIDE the pinned container — no individual absolute positioning', async () => {
        // Core v19 invariant: the container is pinned, the children
        // are in normal flow. A regression that bolted `absolute` back
        // onto the under-row (the v17 mistake, re-applied to the new
        // container shape) would either escape the under-row from the
        // container or stack it on top of the timeline card.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        const queryClient = new QueryClient({
          defaultOptions: { queries: { retry: false } },
        });
        const client = new ApiClient({ baseUrl: 'http://localhost' });
        render(
          <QueryClientProvider client={queryClient}>
            <ApiProvider client={client}>
              <TranscriptPane
                threads={mockThreads}
                activeThread={mockThreads.find((t) => t.id === BRANCH_THREAD_ID)!}
                readOnly={false}
                terminalButton={
                  <button data-testid="terminal-toggle">Terminal</button>
                }
              />
            </ApiProvider>
          </QueryClientProvider>,
        );

        const region = await screen.findByTestId('transcript-top-region');
        const timelineCard = within(region).getByTestId(
          'thread-timeline-overlay',
        );
        const underRow = within(region).getByTestId('transcript-top-row');

        // Container uses a flex column so its two children stack
        // top-to-bottom in normal flow.
        expect(region.className).toContain('flex');
        expect(region.className).toContain('flex-col');

        // Neither child carries its own `absolute` positioning — they
        // are normal-flow children of the pinned container.
        expect(timelineCard.className).not.toContain('absolute');
        expect(underRow.className).not.toContain('absolute');

        // DOM order: timeline card first, then the under-row underneath.
        expect(
          timelineCard.compareDocumentPosition(underRow) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      });

      it("reserves the body's padding-top from --delta-top-region-reserve in the expanded state too (so the first message clears the pinned container)", async () => {
        // v19: the expanded container is absolute (same as the
        // collapsed floating cards), so it takes NO layout space
        // inside the body — the body must reserve a matching
        // `padding-top` from the container's measured height, or the
        // first message renders under the container on initial paint.
        // The mechanism mirrors collapsed: same CSS variable, same
        // inline style on the body. A regression that dropped the
        // expanded reserve (the v18 assumption that "expanded is in
        // flow, no reserve needed") would let the conversation render
        // under the pinned container.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        renderPane(mockThreads, BRANCH_THREAD_ID);

        const region = await screen.findByTestId('transcript-top-region');
        await waitFor(() => expect(lookupObserver(region)).not.toBeNull());

        region.getBoundingClientRect = () => ({ height: 220 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(region)!;
          fire([], fire as unknown as ResizeObserver);
        });

        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('220px'),
        );
        // The body's padding-top references the variable in BOTH
        // states — same mechanism, only the variable's value differs.
        expect(bodyEl().style.paddingTop).toBe(
          'var(--delta-top-region-reserve, 0)',
        );
      });

      it('switches the ResizeObserver target when timelineExpanded flips (collapsed cards → single expanded container)', async () => {
        // Critical v19 mechanism: the ResizeObserver disconnects and
        // re-binds when `timelineExpanded` changes, so the live
        // observation target always matches the rendered state. Going
        // collapsed → expanded must drop the breadcrumb/cluster
        // observation and pick up the container; going back must do
        // the reverse. A leak (observing nodes that no longer exist)
        // or a stale binding (observing the wrong state's nodes)
        // breaks the reserve when the user toggles.
        //
        // Pin the focused session to a real id so the timeline's per-session
        // expand toggle actually persists — under the new-session sentinel
        // the toggle is a no-op (the hook has no id to bind to).
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        renderPane(mockThreads, BRANCH_THREAD_ID);

        // Collapsed initially: the breadcrumb and right cluster are
        // both observed.
        const collapsedBreadcrumb = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        const collapsedCluster = await screen.findByTestId('transcript-top-row');
        expect(collapsedCluster.getAttribute('data-expanded')).toBe('false');
        await waitFor(() =>
          expect(lookupObserver(collapsedBreadcrumb)).not.toBeNull(),
        );
        await waitFor(() =>
          expect(lookupObserver(collapsedCluster)).not.toBeNull(),
        );

        // Toggle to expanded — the collapsed cards unmount, the
        // expanded container mounts. The observer must disconnect
        // from the old nodes and observe the new container.
        const toggle = within(collapsedCluster).getByTestId(
          'thread-timeline-toggle',
        );
        act(() => {
          fireEvent.click(toggle);
        });

        const region = await screen.findByTestId('transcript-top-region');
        expect(region.getAttribute('data-expanded')).toBe('true');

        // Old nodes are gone from observations (the ControllableRO's
        // `disconnect` clears them).
        expect(lookupObserver(collapsedBreadcrumb)).toBeNull();
        expect(lookupObserver(collapsedCluster)).toBeNull();
        // The expanded container is now observed.
        await waitFor(() => expect(lookupObserver(region)).not.toBeNull());

        // Toggle back — re-binds on the collapsed cards.
        const expandedToggle = within(region).getByTestId(
          'thread-timeline-toggle',
        );
        act(() => {
          fireEvent.click(expandedToggle);
        });

        const collapsedClusterAgain = await screen.findByTestId(
          'transcript-top-row',
        );
        await waitFor(() =>
          expect(collapsedClusterAgain.getAttribute('data-expanded')).toBe(
            'false',
          ),
        );
        await waitFor(() =>
          expect(lookupObserver(collapsedClusterAgain)).not.toBeNull(),
        );
        const collapsedBreadcrumbAgain = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        await waitFor(() =>
          expect(lookupObserver(collapsedBreadcrumbAgain)).not.toBeNull(),
        );
      });

      it('drives --delta-top-region-reserve from the max of the breadcrumb and the right cluster heights (collapsed only)', async () => {
        renderPane(mockThreads, BRANCH_THREAD_ID);
        const breadcrumb = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        const rightCluster = await screen.findByTestId('transcript-top-row');
        await waitFor(() =>
          expect(lookupObserver(breadcrumb)).not.toBeNull(),
        );
        await waitFor(() =>
          expect(lookupObserver(rightCluster)).not.toBeNull(),
        );

        // Both cards report their own measured height; the body's
        // reserve is the taller of the two (the visual row height).
        breadcrumb.getBoundingClientRect = () => ({ height: 40 }) as DOMRect;
        rightCluster.getBoundingClientRect = () => ({ height: 72 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(rightCluster)!;
          fire([], fire as unknown as ResizeObserver);
        });

        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('72px'),
        );

        // The right cluster shrinks below the breadcrumb: the reserve
        // tracks the new tallest side.
        rightCluster.getBoundingClientRect = () => ({ height: 28 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(breadcrumb)!;
          fire([], fire as unknown as ResizeObserver);
        });
        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('40px'),
        );
      });

      it('preserves the body\'s scrollTop when the floating cards are measured (no scroll yank)', async () => {
        // Measuring the floating cards must not move the scroll position
        // the user is reading. The observer effect only writes the
        // `--delta-top-region-reserve` CSS variable on the body and
        // never touches `scrollTop`.
        renderPane(mockThreads, BRANCH_THREAD_ID);
        const rightCluster = await screen.findByTestId('transcript-top-row');
        await waitFor(() =>
          expect(lookupObserver(rightCluster)).not.toBeNull(),
        );

        const body = bodyEl();
        Object.defineProperty(body, 'scrollHeight', {
          configurable: true,
          get: () => 4000,
        });
        Object.defineProperty(body, 'clientHeight', {
          configurable: true,
          get: () => 400,
        });
        // Park the user well above the bottom (so stick-to-bottom is OFF):
        // any unintended jump would land at scrollHeight (4000) instead.
        body.scrollTop = 1200;
        fireEvent.scroll(body);

        rightCluster.getBoundingClientRect = () => ({ height: 96 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(rightCluster)!;
          fire([], fire as unknown as ResizeObserver);
        });
        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('96px'),
        );

        // The scroll position the user was reading at stays put.
        expect(body.scrollTop).toBe(1200);
      });
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

describe('TranscriptPane composer context bar', () => {
  beforeEach(() => {
    useNavStore.setState({
      activeThreadId: MAIN_THREAD_ID,
      focusedSessionId: SESSION_ID,
      preNewSessionFocus: null,
    });
    useLiveStore.setState({
      sending: [],
      localSends: {},
      spawns: [],
      notices: {},
      streamingMessages: {},
      runningSubagents: {},
      contextUsage: {},
      rateLimits: null,
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
  });

  it('fills the context bar proportional to the focused session used_percentage', async () => {
    // Seed a second, unfocused session too: the bar must read the FOCUSED
    // session's key (62), not just whatever entry is present, so an off-by-key
    // selector bug would surface here rather than passing on a single entry.
    useLiveStore.setState({
      contextUsage: { [SESSION_ID]: 62, 'other-session': 9 },
    });

    renderPane();

    const card = await screen.findByTestId('composer-card');
    const fill = within(card).getByTestId('composer-context-fill');
    expect(fill).toHaveStyle({ width: '62%' });
    expect(fill).toHaveAttribute('aria-valuenow', '62');
    // The fill is right-anchored along the card's top edge (it grows leftward
    // from the right so its tip stays next to the `%` readout).
    expect(fill.className).toContain('right-0');
    expect(fill.className).toContain('rounded-tr-md');
    expect(within(card).getByTestId('composer-context-bar')).toHaveTextContent(
      '62%',
    );
  });

  it('exposes a focusable label with an always-present help popover', async () => {
    useLiveStore.setState({ contextUsage: { [SESSION_ID]: 62 } });

    renderPane();

    const card = await screen.findByTestId('composer-card');
    // The `%` readout is a focusable, help-cursored span (it reveals the
    // popover on hover/focus rather than via a native `title` tooltip).
    const label = within(card).getByTestId('composer-context-label');
    expect(label).toHaveTextContent('62%');
    expect(label).toHaveAttribute('tabindex', '0');
    expect(label.className).toContain('cursor-help');
    // The old native title tooltip is gone.
    expect(label).not.toHaveAttribute('title');

    // The custom popover is always in the DOM (CSS-toggled on hover/focus), so
    // it is structurally assertable without simulating a hover.
    const popover = within(card).getByTestId('composer-context-popover');
    expect(popover).toHaveAttribute('role', 'note');
    expect(popover).toHaveTextContent('Context window usage');
  });

  it('omits the fill when the focused session has no context usage', async () => {
    // No `contextUsage` entry for the focused session (e.g. no snapshot yet, or
    // null right after /compact).
    renderPane();

    const card = await screen.findByTestId('composer-card');
    expect(
      within(card).queryByTestId('composer-context-fill'),
    ).not.toBeInTheDocument();
    expect(
      within(card).queryByTestId('composer-context-bar'),
    ).not.toBeInTheDocument();
    // With no usage the whole bar is omitted, so neither the label nor its
    // popover is in the DOM.
    expect(
      within(card).queryByTestId('composer-context-label'),
    ).not.toBeInTheDocument();
    expect(
      within(card).queryByTestId('composer-context-popover'),
    ).not.toBeInTheDocument();
  });
});
