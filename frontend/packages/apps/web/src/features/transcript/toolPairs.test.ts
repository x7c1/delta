import { describe, expect, it } from 'vitest';
import type { ContentBlock, Message } from '@delta/model';
import { buildToolPairing, messageRendersNothing } from './toolPairs';

function message(uuid: string, content: ContentBlock[]): Message {
  return {
    uuid,
    session_id: 's',
    thread_id: 't',
    role: 'assistant',
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq: 0,
    content_text: null,
    content,
    created_at: '2026-01-01T00:00:00Z',
  };
}

describe('buildToolPairing', () => {
  it('links a tool_use to its result across separate messages', () => {
    const messages = [
      message('a', [
        { type: 'tool_use', id: 't1', name: 'ToolSearch', input: { q: 'x' } },
      ]),
      message('u', [
        { type: 'tool_result', tool_use_id: 't1', content: 'hits', is_error: false },
      ]),
    ];

    const pairing = buildToolPairing(messages);
    expect(pairing.toolUseIds.has('t1')).toBe(true);
    expect(pairing.resultByUseId.get('t1')?.content).toBe('hits');
  });

  it('leaves an unmatched result unpaired', () => {
    const messages = [
      message('u', [
        { type: 'tool_result', tool_use_id: 'gone', content: 'x', is_error: false },
      ]),
    ];
    const pairing = buildToolPairing(messages);
    expect(pairing.toolUseIds.has('gone')).toBe(false);
    expect(pairing.resultByUseId.has('gone')).toBe(true);
  });
});

describe('messageRendersNothing', () => {
  const messages = [
    message('a', [
      { type: 'tool_use', id: 't1', name: 'ToolSearch', input: {} },
    ]),
    message('u', [
      { type: 'tool_result', tool_use_id: 't1', content: 'hits', is_error: false },
    ]),
  ];
  const pairing = buildToolPairing(messages);

  it('skips a message that is only paired tool results', () => {
    expect(messageRendersNothing(messages[1], pairing)).toBe(true);
  });

  it('skips a message that is only an empty thinking block', () => {
    // Claude Code stores a signed reference but no plaintext for thinking.
    const empty = message('th', [{ type: 'thinking', thinking: '   ' }]);
    expect(messageRendersNothing(empty, buildToolPairing([empty]))).toBe(true);
  });

  it('skips a message with no content', () => {
    const blank = message('z', []);
    expect(messageRendersNothing(blank, buildToolPairing([blank]))).toBe(true);
  });

  it('keeps a message whose result is an orphan', () => {
    const orphan = message('o', [
      { type: 'tool_result', tool_use_id: 'gone', content: 'x', is_error: false },
    ]);
    expect(messageRendersNothing(orphan, buildToolPairing([orphan]))).toBe(
      false,
    );
  });

  it('keeps a message that also has text', () => {
    const mixed = message('m', [
      { type: 'tool_result', tool_use_id: 't1', content: 'hits', is_error: false },
      { type: 'text', text: 'and here is what I found' },
    ]);
    expect(messageRendersNothing(mixed, pairing)).toBe(false);
  });

  it('keeps a message with a non-empty thinking block', () => {
    const thinking = message('t', [{ type: 'thinking', thinking: 'pondering' }]);
    expect(
      messageRendersNothing(thinking, buildToolPairing([thinking])),
    ).toBe(false);
  });

  it('keeps a message with a tool_use', () => {
    expect(messageRendersNothing(messages[0], pairing)).toBe(false);
  });
});
