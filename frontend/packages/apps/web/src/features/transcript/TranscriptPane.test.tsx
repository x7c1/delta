/**
 * Conversation rendering: the fetched messages, the breadcrumb and branch
 * chips, and which rows render (and how they are indented).
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
import type { MessagesResponse } from '@delta/wire-gen';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { TranscriptPane } from './TranscriptPane';
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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

  it('renders the unknown-command system notice (the one user-facing system row)', async () => {
    // Claude Code rejects an unrecognized slash command with a `role: "system"`
    // line carrying `Unknown command: <command>`. Unlike a generic system row
    // (see the test above), this one is surfaced so the user learns the command
    // was rejected instead of seeing nothing.
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
              content_text: '/review-pr',
              content: [{ type: 'text', text: '/review-pr' }],
              created_at: '2026-01-01T00:00:01Z',
              ...NO_METADATA,
            },
            {
              uuid: 'm-unknown-cmd',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'system',
              linear_parent_uuid: 'm-user',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: 'Unknown command: /review-pr',
              content: [{ type: 'text', text: 'Unknown command: /review-pr' }],
              created_at: '2026-01-01T00:00:02Z',
              ...NO_METADATA,
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane();

    await waitFor(() =>
      expect(
        screen.getByText('Unknown command: /review-pr'),
      ).toBeInTheDocument(),
    );
    const noticeItem = screen
      .getByText('Unknown command: /review-pr')
      .closest('[data-testid="message-item"]');
    expect(noticeItem).toHaveAttribute('data-role', 'system');
    // It reads as a nested system aside, not prose: its block wrapper carries
    // the same `ml-6` left indent as meta lines and tool rows.
    expect(noticeItem?.parentElement?.className).toContain('ml-6');
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
              ...NO_METADATA,
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
              ...NO_METADATA,
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

  it('nests a reasoning message with the tool cards and leaves a prose reply at full width', async () => {
    // The nesting decision is driven by the shape a message renders as, not by a
    // list of tool block types: a message whose only block is the model's
    // reasoning renders as a card, so it must read as a nested step exactly like
    // a tool row. Asserted here, at the layer that applies the class — the block
    // wrapper belongs to this component, so a message-level test cannot see it.
    // (The task-notification, meta and unknown-command asides keep their own
    // nesting coverage in the tests above; they carry prose and nest by
    // category, not by shape.)
    server.use(
      http.get('*/api/threads/:id/messages', () => {
        const body: MessagesResponse = {
          messages: [
            {
              uuid: 'm-reply',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: null,
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 0,
              content_text: 'the answer',
              content: [{ type: 'text', text: 'the answer' }],
              created_at: '2026-01-01T00:00:01Z',
              ...NO_METADATA,
            },
            {
              uuid: 'm-reasoning',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-reply',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 1,
              content_text: 'weighing the options',
              content: [{ type: 'thinking', thinking: 'weighing the options' }],
              created_at: '2026-01-01T00:00:02Z',
              ...NO_METADATA,
            },
            {
              uuid: 'm-tool',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-reasoning',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 2,
              content_text: null,
              content: [
                { type: 'tool_use', id: 't1', name: 'Bash', input: {} },
              ],
              created_at: '2026-01-01T00:00:03Z',
              ...NO_METADATA,
            },
            {
              uuid: 'm-reply-with-tool',
              session_id: 's',
              thread_id: MAIN_THREAD_ID,
              role: 'assistant',
              linear_parent_uuid: 'm-tool',
              semantic_parent_uuid: null,
              prompt_id: null,
              seq: 3,
              content_text: 'running it now',
              content: [
                { type: 'text', text: 'running it now' },
                { type: 'tool_use', id: 't2', name: 'Bash', input: {} },
              ],
              created_at: '2026-01-01T00:00:04Z',
              ...NO_METADATA,
            },
          ],
        };
        return HttpResponse.json(body);
      }),
    );

    renderPane();

    const blockOf = async (uuid: string) => {
      const item = await waitFor(() => {
        const el = document.querySelector(`[data-message-uuid="${uuid}"]`);
        expect(el).not.toBeNull();
        return el!;
      });
      return item.parentElement!;
    };

    // The reasoning message is a nested step: left-indented and tightened —
    // byte-for-byte the layout the tool row beside it gets.
    const reasoning = await blockOf('m-reasoning');
    const tool = await blockOf('m-tool');
    expect(reasoning.className).toContain('ml-6');
    expect(reasoning.className).toContain('pt-0.5');
    expect(reasoning.className).toContain('pb-0.5');
    expect(reasoning.className).toBe(tool.className);

    // A prose reply keeps the full-width prose layout.
    const reply = await blockOf('m-reply');
    expect(reply.className).not.toContain('ml-6');
    expect(reply.className).not.toContain('pt-0.5');

    // A reply that also carries a tool call still nests (regression guard: it
    // nested before via the tool-block check and must keep doing so).
    const replyWithTool = await blockOf('m-reply-with-tool');
    expect(replyWithTool.className).toContain('ml-6');
  });
});
