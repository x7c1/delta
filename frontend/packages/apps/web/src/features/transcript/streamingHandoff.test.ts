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
  it('matches when the last assistant message has the same visible text', () => {
    const messages = [
      message('user', [{ type: 'text', text: 'hi' }], 'u'),
      assistantText('Streaming this reply live.', 'a'),
    ];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.', false),
    ).toBe(true);
  });

  it('ignores surrounding whitespace on both sides', () => {
    const messages = [assistantText('  Streaming this reply live.\n')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.  ', false),
    ).toBe(true);
  });

  it('prefix-matches the persisted completed version only when the stream is final', () => {
    // A late final delta can leave the buffer one chunk short of the flushed
    // line; once the stream is final, `streamed` is the complete text, so the
    // persisted (authoritative) copy is allowed to prefix-match.
    const messages = [assistantText('Streaming this reply live.')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply', true),
    ).toBe(true);
  });

  it('does NOT prefix-match a still-growing (non-final) stream', () => {
    // While streaming, `streamed` is a growing prefix; allowing startsWith here
    // would hide a genuinely in-flight bubble whose final form differs.
    const messages = [assistantText('Streaming this reply live.')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply', false),
    ).toBe(false);
  });

  it('does NOT match an EARLIER assistant message sharing a prefix with the partial stream', () => {
    // The false-positive guard: an in-flight stream is a growing prefix and can
    // collide with a prior assistant turn that opens the same way (e.g.
    // "Let me…"). `startsWith` is gated on `streamComplete`, so while the stream
    // is still partial (non-final) the earlier message must not suppress the
    // live bubble even though the scan now covers all assistant messages.
    const messages = [
      message('user', [{ type: 'text', text: 'first question' }], 'u1'),
      assistantText('Let me check that for you. Done — answer one.', 'a1'),
      message('user', [{ type: 'text', text: 'second question' }], 'u2'),
    ];
    // Streaming the second reply, which so far shares the "Let me check" opener.
    expect(persistedHasStreamedText(messages, 'Let me check', false)).toBe(
      false,
    );
  });

  it('suppresses once an assistant message trimmed-equals the partial stream', () => {
    // Even with a prefix-colliding earlier message present, the moment the new
    // reply is persisted and equals the stream, the bubble is suppressed.
    const reply = 'Let me check that — answer two.';
    const messages = [
      assistantText('Let me check that for you. Answer one.', 'a1'),
      message('user', [{ type: 'text', text: 'second question' }], 'u2'),
      assistantText(reply, 'a2'),
    ];
    expect(persistedHasStreamedText(messages, reply, false)).toBe(true);
  });

  it('matches the text-block message even when a later tool_use message is last (tool turn)', () => {
    // Regression: Claude splits a single assistant reply into separate
    // per-content-block transcript lines. In a tool turn the visible text lives
    // in an EARLIER assistant message (the text-block line) while the LAST
    // assistant message is a tool_use-only line with no visible text. The scan
    // must find the text-block line regardless of its position.
    const reply = 'Here is the answer before calling a tool.';
    const messages = [
      message('user', [{ type: 'text', text: 'do a thing' }], 'u'),
      assistantText(reply, 'a-text'),
      message(
        'assistant',
        [{ type: 'tool_use', id: 't1', name: 'Bash', input: { command: 'ls' } }],
        'a-tool',
      ),
    ];
    expect(persistedHasStreamedText(messages, reply, false)).toBe(true);
    expect(persistedHasStreamedText(messages, reply, true)).toBe(true);
  });

  it('matches an earlier text-block message when a later assistant message has DIFFERENT text', () => {
    // After a tool_result, a subsequent assistant message carries different
    // visible text (e.g. the follow-up after the tool ran). The originally
    // streamed text still matches its own earlier text-block line.
    const reply = 'Here is the answer before calling a tool.';
    const messages = [
      assistantText(reply, 'a-text'),
      message(
        'assistant',
        [{ type: 'tool_use', id: 't1', name: 'Bash', input: { command: 'ls' } }],
        'a-tool',
      ),
      message(
        'user',
        [
          {
            type: 'tool_result',
            tool_use_id: 't1',
            content: 'output',
            is_error: false,
          },
        ],
        'u-result',
      ),
      assistantText('A different follow-up after the tool ran.', 'a-followup'),
    ];
    expect(persistedHasStreamedText(messages, reply, false)).toBe(true);
  });

  it('does not match while the reply is still streaming and unpersisted', () => {
    // Only the user turn is persisted; the assistant reply is not yet a line.
    const messages = [message('user', [{ type: 'text', text: 'hi' }], 'u')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.', false),
    ).toBe(false);
  });

  it('never matches empty streamed text', () => {
    const messages = [assistantText('Streaming this reply live.')];
    expect(persistedHasStreamedText(messages, '', false)).toBe(false);
    expect(persistedHasStreamedText(messages, '   ', true)).toBe(false);
  });

  it('only considers assistant-role messages', () => {
    // A user message echoing the same text must not suppress the bubble.
    const messages = [
      message('user', [{ type: 'text', text: 'Streaming this reply live.' }], 'u'),
    ];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.', false),
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
      persistedHasStreamedText(messages, 'Streaming this reply live.', false),
    ).toBe(true);
  });

  it('does not match a different assistant reply', () => {
    const messages = [assistantText('A completely different answer.')];
    expect(
      persistedHasStreamedText(messages, 'Streaming this reply live.', false),
    ).toBe(false);
  });
});
