/**
 * The live streaming bubble and its hand-off to the persisted message, and
 * the thread gating of the bubble and the question card.
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
import { act, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import type { MessagesResponse } from '@delta/wire-gen';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  SESSION_ID,
  mockThreads,
} from '@delta/api-mocks';
import { useLiveStore } from '../../store/liveStore';
import {
  NO_METADATA,
  renderPane,
  resetTranscriptPaneState,
  server,
} from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane', () => {
  beforeEach(resetTranscriptPaneState);

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
        thread_id: MAIN_THREAD_ID,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
});
