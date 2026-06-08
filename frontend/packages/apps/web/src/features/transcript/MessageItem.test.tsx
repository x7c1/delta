import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ContentBlock, Message, MessageRole } from '@delta/model';
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
    const expected = formatLocalDateTime('2026-01-01T00:00:00Z');
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

  it('renders no timestamp when created_at is unparseable', () => {
    const message = { ...makeMessage('user', 'hi'), created_at: 'not-a-date' };
    render(<MessageItem message={message} />);
    expect(screen.queryByText(/\d{4}-\d{2}-\d{2}/)).toBeNull();
  });
});
