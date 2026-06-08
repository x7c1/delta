import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { Message, MessageRole } from '@delta/model';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { MessageItem } from './MessageItem';

function makeMessage(role: MessageRole, text: string): Message {
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
    content: [{ type: 'text', text }],
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

  it('renders no timestamp when created_at is unparseable', () => {
    const message = { ...makeMessage('user', 'hi'), created_at: 'not-a-date' };
    render(<MessageItem message={message} />);
    expect(screen.queryByText(/\d{4}-\d{2}-\d{2}/)).toBeNull();
  });
});
