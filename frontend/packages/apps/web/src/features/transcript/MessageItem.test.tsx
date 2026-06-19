import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import type { ContentBlock, Message, MessageRole } from '@delta/wire-gen';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { MessageItem } from './MessageItem';

function makeMessage(role: MessageRole, text: string): Message {
  return makeMessageWithContent(role, [{ type: 'text', text }], text);
}

function makeMessageWithContent(
  role: MessageRole,
  content: ContentBlock[],
  text: string | null = null,
): Message {
  return {
    uuid: 'm-1',
    session_id: 's',
    thread_id: 't',
    role,
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq: 0,
    content_text: text,
    content,
    created_at: '2026-01-01T00:00:00Z',
    model: null,
    git_branch: null,
    cwd: null,
    response_time_ms: null,
  };
}

describe('MessageItem', () => {
  it('renders user text verbatim, preserving newlines and literal Markdown', () => {
    render(<MessageItem message={makeMessage('user', 'line one\nline two *x*')} />);

    // The exact text — including the newline and the literal `*x*` — is
    // rendered as a single text node, i.e. not run through Markdown.
    // `normalizer: (s) => s` keeps the newline instead of collapsing it.
    const node = screen.getByText('line one\nline two *x*', {
      normalizer: (s) => s,
    });
    expect(node).toHaveClass('whitespace-pre-wrap');
    // `*x*` stays literal: no emphasis element is produced for user text.
    expect(node.querySelector('em')).toBeNull();
  });

  it('renders assistant text through Markdown', () => {
    render(<MessageItem message={makeMessage('assistant', 'hello **bold**')} />);

    const strong = screen.getByText('bold');
    expect(strong.tagName).toBe('STRONG');
  });

  it('renders GFM tables in assistant text', () => {
    const table = '| A | B |\n|---|---|\n| 1 | 2 |';
    render(<MessageItem message={makeMessage('assistant', table)} />);

    // Without remark-gfm the pipes render as a literal paragraph; with it the
    // markdown becomes a real <table> with header cells.
    expect(screen.getByRole('table')).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'A' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '2' })).toBeInTheDocument();
  });

  it('renders the local-time timestamp for both roles', () => {
    // The transcript shows seconds (unlike the session list).
    const expected = formatLocalDateTime('2026-01-01T00:00:00Z', true);
    expect(expected).not.toBeNull();

    for (const role of ['user', 'assistant'] as const) {
      const { unmount } = render(
        <MessageItem message={makeMessage(role, 'hi')} />,
      );
      const stamp = screen.getByText(expected as string);
      expect(stamp).toHaveClass('tabular-nums');
      unmount();
    }
  });

  it('distinguishes the sender by shape, not a role label', () => {
    const { rerender } = render(
      <MessageItem message={makeMessage('user', 'hi')} />,
    );
    // No "You"/"Assistant" labels — the sender is conveyed by layout alone.
    expect(screen.queryByText('You')).toBeNull();
    expect(screen.queryByText('Assistant')).toBeNull();
    // The user turn is data-tagged so the layout can be asserted structurally.
    expect(screen.getByTestId('message-item')).toHaveAttribute(
      'data-role',
      'user',
    );

    rerender(<MessageItem message={makeMessage('assistant', 'hi')} />);
    expect(screen.queryByText('Assistant')).toBeNull();
    expect(screen.getByTestId('message-item')).toHaveAttribute(
      'data-role',
      'assistant',
    );
  });

  it('renders a user-role tool_result on the assistant side, not as a user bubble', () => {
    // Claude returns tool results as `role: "user"` lines. Such a message
    // carries no author-written text, so it must not be laid out as a
    // right-aligned user bubble.
    const message = makeMessageWithContent('user', [
      {
        type: 'tool_result',
        tool_use_id: 't1',
        content: 'files: a, b',
        is_error: false,
      },
    ]);
    render(<MessageItem message={message} />);

    // Not right-aligned: the assistant-side layout omits the bubble wrapper.
    expect(screen.getByTestId('message-item')).not.toHaveClass('items-end');
  });

  it('still right-aligns a genuine user turn that has text', () => {
    render(<MessageItem message={makeMessage('user', 'hi')} />);
    expect(screen.getByTestId('message-item')).toHaveClass('items-end');
  });

  it('renders a tool call together with its paired result', () => {
    const message = makeMessageWithContent('assistant', [
      { type: 'tool_use', id: 't1', name: 'ToolSearch', input: { q: 'x' } },
    ]);
    const pairing = {
      toolUseIds: new Set(['t1']),
      resultByUseId: new Map([
        [
          't1',
          {
            type: 'tool_result' as const,
            tool_use_id: 't1',
            content: 'search-hits',
            is_error: false,
          },
        ],
      ]),
    };
    render(<MessageItem message={message} pairing={pairing} />);

    // The tool name heads the (collapsed) row; expanding reveals the result.
    const toggle = screen.getByRole('button', { name: /ToolSearch/ });
    expect(toggle).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(screen.getByText('search-hits')).toBeInTheDocument();
  });

  it('suppresses a tool_result whose call is shown elsewhere', () => {
    const message = makeMessageWithContent('user', [
      {
        type: 'tool_result',
        tool_use_id: 't1',
        content: 'search-hits',
        is_error: false,
      },
    ]);
    const pairing = {
      toolUseIds: new Set(['t1']),
      resultByUseId: new Map(),
    };
    render(<MessageItem message={message} pairing={pairing} />);

    // The paired result is rendered inline with its call, not as its own block.
    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.queryByText('search-hits')).toBeNull();
  });

  it('omits an empty thinking block', () => {
    // Claude Code leaves the thinking plaintext empty (only a signature), so
    // an empty thinking block must not render a click-to-empty collapsible.
    const message = makeMessageWithContent('assistant', [
      { type: 'thinking', thinking: '   ' },
      { type: 'text', text: 'the answer' },
    ]);
    render(<MessageItem message={message} />);

    expect(screen.queryByText('thinking')).toBeNull();
    expect(screen.getByText('the answer')).toBeInTheDocument();
  });

  it('renders nothing for a message whose only block is empty thinking', () => {
    // Hiding the empty thinking block must not leave a bare timestamp behind:
    // the whole turn collapses to nothing and is dropped.
    const message = makeMessageWithContent('assistant', [
      { type: 'thinking', thinking: '' },
    ]);
    const { container } = render(<MessageItem message={message} />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId('message-item')).toBeNull();
  });

  it('renders a thinking block that has text', () => {
    const message = makeMessageWithContent('assistant', [
      { type: 'thinking', thinking: 'let me reason' },
    ]);
    render(<MessageItem message={message} />);
    expect(screen.getByText('thinking')).toBeInTheDocument();
  });

  it('renders a meta line as a collapsed card, not a user bubble', () => {
    // Harness-injected lines (skill bodies, system reminders) arrive as
    // `role: "meta"`. They must render collapsed on the assistant side, never
    // as a right-aligned user bubble, and the body must stay hidden until the
    // disclosure is toggled.
    // A multi-line body so the collapsed summary (first line only) is distinct
    // from the hidden body (a later line), letting us assert the body is hidden.
    const summaryLine = '<system-reminder> injected skill body';
    const hiddenLine = 'BODY LINE ONLY VISIBLE WHEN EXPANDED';
    const message = makeMessage('meta', `${summaryLine}\n${hiddenLine}`);
    render(<MessageItem message={message} />);

    const item = screen.getByTestId('message-item');
    expect(item).toHaveAttribute('data-role', 'meta');
    // Not a right-aligned user bubble.
    expect(item).not.toHaveClass('items-end');

    // Collapsed by default: the summary's first line shows, but the rest of the
    // body is not in the DOM until toggled.
    const toggle = screen.getByRole('button');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    const summaryNode = screen.getByText(summaryLine);
    expect(summaryNode).toBeInTheDocument();
    // The first-line text truncates so a long meta line cannot overflow the
    // collapsed card; it shrinks within the flex button instead of clipping.
    expect(summaryNode).toHaveClass('truncate');
    expect(screen.queryByText(new RegExp(hiddenLine))).toBeNull();

    // Toggling reveals the verbatim body.
    fireEvent.click(toggle);
    expect(screen.getByText(new RegExp(hiddenLine))).toBeInTheDocument();
  });

  it('folds a task-notification user row into a collapsed card', () => {
    // A background-task completion arrives as a normal `role: "user"` line
    // whose text starts with `<task-notification>` (not a meta row). It must
    // render collapsed on the assistant side — never a right-aligned user
    // bubble — with the body hidden until the disclosure is toggled.
    const summaryBadge = 'task notification';
    const hiddenLine = 'BODY LINE ONLY VISIBLE WHEN EXPANDED';
    const body = `<task-notification>\nbackground task finished\n${hiddenLine}\n</task-notification>`;
    render(<MessageItem message={makeMessage('user', body)} />);

    const item = screen.getByTestId('message-item');
    expect(item).toHaveAttribute('data-task-notification', 'true');
    // Not a right-aligned user bubble.
    expect(item).not.toHaveClass('items-end');

    // Collapsed by default: the badge summary shows, but the body is not in the
    // DOM until toggled.
    const toggle = screen.getByRole('button');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByText(summaryBadge)).toBeInTheDocument();
    expect(screen.queryByText(new RegExp(hiddenLine))).toBeNull();

    // Toggling reveals the verbatim body.
    fireEvent.click(toggle);
    expect(screen.getByText(new RegExp(hiddenLine))).toBeInTheDocument();
  });

  it('detects a task-notification with leading whitespace, mirroring the backend', () => {
    // The backend trims leading whitespace before the prefix check; the fold
    // must match the same shape.
    render(<MessageItem message={makeMessage('user', '  <task-notification>done')} />);
    expect(screen.getByTestId('message-item')).toHaveAttribute(
      'data-task-notification',
      'true',
    );
  });

  it('leaves a normal user turn unfolded (no task-notification card)', () => {
    // A plain user turn must stay a right-aligned bubble, not be folded.
    render(<MessageItem message={makeMessage('user', 'just a normal message')} />);
    const item = screen.getByTestId('message-item');
    expect(item).not.toHaveAttribute('data-task-notification');
    expect(item).toHaveClass('items-end');
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('renders no timestamp when created_at is unparseable', () => {
    const message = { ...makeMessage('user', 'hi'), created_at: 'not-a-date' };
    render(<MessageItem message={message} />);
    expect(screen.queryByText(/\d{4}-\d{2}-\d{2}/)).toBeNull();
  });

  function assistantWithMeta(): Message {
    return {
      ...makeMessage('assistant', 'an answer'),
      model: 'Opus 4.8',
      cwd: '/home/dev/repo',
      git_branch: 'feature/meta',
      response_time_ms: 9400,
    };
  }

  it('renders the latest assistant message with the full meta row', () => {
    render(<MessageItem message={assistantWithMeta()} isLatest />);

    // The right column surfaces the timestamp and the model rendered as
    // `<model> in <responseTime>`.
    const model = screen.getByTestId('meta-model');
    expect(model).toHaveTextContent('Opus 4.8');
    expect(model).toHaveTextContent('in');
    expect(screen.getByTestId('meta-response-time')).toHaveTextContent('9.4s');
    const expected = formatLocalDateTime('2026-01-01T00:00:00Z', true) as string;
    expect(screen.getByText(expected)).toBeInTheDocument();

    // The left column surfaces the working location: cwd (home-collapsed) and
    // the branch name (the `⑂` glyph was removed).
    expect(screen.getByTestId('meta-cwd')).toHaveTextContent('~/repo');
    const branch = screen.getByTestId('meta-branch');
    expect(branch).toHaveTextContent('feature/meta');
    expect(branch.textContent).not.toContain('⑂');
  });

  it('renders an older assistant message with only the timestamp', () => {
    render(<MessageItem message={assistantWithMeta()} isLatest={false} />);

    // No model, cwd, branch, or response time rendered inline for an older
    // message…
    expect(screen.queryByTestId('meta-model')).toBeNull();
    expect(screen.queryByTestId('meta-cwd')).toBeNull();
    expect(screen.queryByTestId('meta-branch')).toBeNull();
    expect(screen.queryByTestId('meta-response-time')).toBeNull();
    // …just the timestamp, which doubles as the popover trigger.
    const expected = formatLocalDateTime('2026-01-01T00:00:00Z', true) as string;
    expect(screen.getByText(expected)).toBeInTheDocument();
    expect(screen.getByTestId('meta-time')).toBeInTheDocument();
  });

  it('appends the response time after the model only on the latest message', () => {
    // The response time is no longer in the popover; it rides the model line as
    // `<model> in <responseTime>`, and only on the latest message.
    render(<MessageItem message={assistantWithMeta()} isLatest />);
    const model = screen.getByTestId('meta-model');
    expect(model).toHaveTextContent('Opus 4.8 in 9.4s');
  });

  it('shows the model without a response time when none was captured', () => {
    const message: Message = {
      ...assistantWithMeta(),
      response_time_ms: null,
    };
    render(<MessageItem message={message} isLatest />);

    const model = screen.getByTestId('meta-model');
    expect(model).toHaveTextContent('Opus 4.8');
    expect(model.textContent).not.toContain('in');
    expect(screen.queryByTestId('meta-response-time')).toBeNull();
  });

  it('degrades the latest meta when cwd and branch are absent', () => {
    // A latest assistant message that carries only the model (no cwd/branch)
    // must still render the model, and must NOT render empty cwd/branch cells.
    const message: Message = {
      ...assistantWithMeta(),
      cwd: null,
      git_branch: null,
    };
    render(<MessageItem message={message} isLatest />);

    expect(screen.getByTestId('meta-model')).toHaveTextContent('Opus 4.8');
    expect(screen.getByTestId('meta-response-time')).toHaveTextContent('9.4s');
    // No cwd or branch cell at all when both are missing.
    expect(screen.queryByTestId('meta-cwd')).toBeNull();
    expect(screen.queryByTestId('meta-branch')).toBeNull();
  });

  it('renders the latest branch when only the branch is present', () => {
    // Partial location: a branch but no cwd. Only the branch cell renders, with
    // no dangling cwd cell.
    const message: Message = {
      ...assistantWithMeta(),
      cwd: null,
      git_branch: 'feature/only-branch',
    };
    render(<MessageItem message={message} isLatest />);

    expect(screen.queryByTestId('meta-cwd')).toBeNull();
    expect(screen.getByTestId('meta-branch')).toHaveTextContent('feature/only-branch');
  });

  it('shows em dashes in the popover for the message metadata that is absent', () => {
    // A message missing every metadata field still renders the popover with each
    // labelled row present, falling back to an em dash rather than crashing or
    // omitting the row. There is no response-time row in the popover anymore.
    render(<MessageItem message={makeMessage('assistant', 'an answer')} isLatest={false} />);

    const popover = screen.getByTestId('message-meta-popover');
    expect(within(popover).getByTestId('popover-model')).toHaveTextContent('—');
    expect(within(popover).getByTestId('popover-cwd')).toHaveTextContent('—');
    expect(within(popover).getByTestId('popover-branch')).toHaveTextContent('—');
    // The popover no longer carries a response-time row.
    expect(within(popover).queryByTestId('popover-time')).toBeNull();
  });

  it('popover lists model, cwd (home-collapsed) and branch — no response time or token/cache figures', () => {
    render(<MessageItem message={assistantWithMeta()} isLatest={false} />);

    const popover = screen.getByTestId('message-meta-popover');
    expect(popover).toHaveTextContent('model');
    expect(within(popover).getByTestId('popover-model')).toHaveTextContent('Opus 4.8');
    expect(popover).toHaveTextContent('cwd');
    expect(within(popover).getByTestId('popover-cwd')).toHaveTextContent('~/repo');
    expect(popover).toHaveTextContent('branch');
    expect(within(popover).getByTestId('popover-branch')).toHaveTextContent('feature/meta');

    // The response time moved out of the popover onto the latest message's model
    // line, so it must not appear here.
    expect(popover).not.toHaveTextContent('response time');
    expect(popover.textContent).not.toContain('9.4s');

    // The popover is intentionally limited to those three facts: no token counts
    // or cache ratios leak into it.
    expect(popover.textContent).not.toMatch(/token/i);
    expect(popover.textContent).not.toMatch(/cache/i);
    expect(popover.textContent).not.toMatch(/cost/i);
  });
});
