import { describe, expect, it } from 'vitest';
import type { ContentBlock, Message } from '@delta/wire-gen';
import { persistedHasStreamedText } from './streamingHandoff';

function message(
  role: Message['role'],
  content: ContentBlock[],
  uuid = 'm',
): Message {
  return {
    uuid,
    session_id: 's',
    thread_id: 1,
    role,
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq: 0,
    content_text: '',
    content,
    created_at: '2026-01-01T00:00:00Z',
  };
}

function assistantText(text: string, uuid?: string): Message {
  return message('assistant', [{ type: 'text', text }], uuid);
}

describe('persistedHasStreamedText', () => {
  it('matches when an assistant message has the same visible text', () => {
    const messages = [
      message('user', [{ type: 'text', text: 'hi' }], 'u'),
      assistantText('Streaming this reply live.', 'a'),
    ];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.'),
    ).toBe(true);
  });

  it('ignores surrounding whitespace on both sides', () => {
    const messages = [assistantText('  Streaming this reply live.\n')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.  '),
    ).toBe(true);
  });

  it('matches when the persisted text is the completed version of the stream', () => {
    // A late final delta can leave the buffer one chunk short of the flushed
    // line; the persisted (complete) copy is authoritative, so prefix-match.
    const messages = [assistantText('Streaming this reply live.')];
    expect(persistedHasStreamedText(messages, 'Streaming this reply')).toBe(
      true,
    );
  });

  it('does not match while the reply is still streaming and unpersisted', () => {
    // Only the user turn is persisted; the assistant reply is not yet a line.
    const messages = [message('user', [{ type: 'text', text: 'hi' }], 'u')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.'),
    ).toBe(false);
  });

  it('never matches empty streamed text', () => {
    const messages = [assistantText('Streaming this reply live.')];
    expect(persistedHasStreamedText(messages, '')).toBe(false);
    expect(persistedHasStreamedText(messages, '   ')).toBe(false);
  });

  it('only considers assistant-role messages', () => {
    // A user message echoing the same text must not suppress the bubble.
    const messages = [
      message('user', [{ type: 'text', text: 'Streaming this reply live.' }], 'u'),
    ];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.'),
    ).toBe(false);
  });

  it('reads only text blocks, skipping thinking and tool blocks', () => {
    const messages = [
      message(
        'assistant',
        [
          { type: 'thinking', thinking: 'planning the answer' },
          { type: 'tool_use', id: 't1', name: 'Bash', input: { command: 'ls' } },
          { type: 'text', text: 'Streaming this reply live.' },
        ],
        'a',
      ),
    ];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.'),
    ).toBe(true);
  });

  it('does not match a different assistant reply', () => {
    const messages = [assistantText('A completely different answer.')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.'),
    ).toBe(false);
  });
});
