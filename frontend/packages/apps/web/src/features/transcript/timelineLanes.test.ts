import { describe, expect, it } from 'vitest';
import type { Message, Thread } from '@delta/wire-gen';
import {
  MAIN_LANE_LABEL,
  buildSortedMessages,
  buildTimelineLanes,
  classifyMessage,
  computeTimeRange,
  findNearestMessageIndex,
  messageTimeMs,
  xFraction,
} from './timelineLanes';

function thread(
  id: number,
  {
    parent = null,
    rootUuid = null,
    createdAt = '2026-01-01T00:00:00Z',
    title,
  }: {
    parent?: number | null;
    rootUuid?: string | null;
    createdAt?: string;
    title?: string;
  } = {},
): Thread {
  return {
    id,
    session_id: 'session-1',
    title: title ?? `thread ${id}`,
    parent_thread_id: parent,
    root_message_uuid: rootUuid,
    created_at: createdAt,
  };
}

function message(
  threadId: number,
  seq: number,
  uuid?: string,
  {
    createdAt = '2026-01-01T00:00:00Z',
    contentText = null as string | null,
    content = [] as Message['content'],
    role = 'user' as Message['role'],
  } = {},
): Message {
  return {
    uuid: uuid ?? `m-${threadId}-${seq}`,
    session_id: 'session-1',
    thread_id: threadId,
    role,
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq,
    content_text: contentText,
    content,
    created_at: createdAt,
    model: null,
    git_branch: null,
    cwd: null,
    response_time_ms: null,
  };
}

describe('classifyMessage', () => {
  it('classifies a user-role message with author text as "user"', () => {
    const m = message(1, 0, 'm', {
      role: 'user',
      content: [{ type: 'text', text: 'hello' }],
    });
    expect(classifyMessage(m)).toBe('user');
  });

  it('classifies a user-role message that only carries tool results as "other"', () => {
    // Matches MessageItem's left/right split: a tool-result carrier renders
    // on the assistant side, so it counts as "other" here too.
    const m = message(1, 0, 'm', {
      role: 'user',
      content: [
        { type: 'tool_result', tool_use_id: 't', content: '', is_error: false },
      ],
    });
    expect(classifyMessage(m)).toBe('other');
  });

  it('classifies assistant, meta, system, and other roles as "other"', () => {
    expect(
      classifyMessage(
        message(1, 0, 'a', { role: 'assistant', content: [{ type: 'text', text: 'hi' }] }),
      ),
    ).toBe('other');
    expect(
      classifyMessage(
        message(1, 0, 'm', { role: 'meta', content: [{ type: 'text', text: 'sys' }] }),
      ),
    ).toBe('other');
    expect(
      classifyMessage(
        message(1, 0, 's', { role: 'system', content: [{ type: 'text', text: 'sys' }] }),
      ),
    ).toBe('other');
    expect(
      classifyMessage(
        message(1, 0, 'o', { role: 'other', content: [{ type: 'text', text: 'x' }] }),
      ),
    ).toBe('other');
  });
});

describe('messageTimeMs', () => {
  it('parses a populated ISO-8601 timestamp into epoch ms', () => {
    const m = message(1, 0, 'm1', { createdAt: '2026-01-01T00:00:00Z' });
    expect(messageTimeMs(m)).toBe(Date.parse('2026-01-01T00:00:00Z'));
  });

  it('returns null for the wire contract empty-string sentinel', () => {
    const m = message(1, 0, 'm1', { createdAt: '' });
    expect(messageTimeMs(m)).toBeNull();
  });

  it('returns null for an unparseable timestamp', () => {
    const m = message(1, 0, 'm1', { createdAt: 'not-a-date' });
    expect(messageTimeMs(m)).toBeNull();
  });
});

describe('computeTimeRange', () => {
  it('returns the earliest and latest epoch ms across every lane', () => {
    const messages = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '2026-01-01T00:05:00Z' }),
        ],
      ],
      [
        2,
        [
          message(2, 0, 'c', { createdAt: '2026-01-01T00:02:00Z' }),
          message(2, 1, 'd', { createdAt: '2026-01-01T00:08:00Z' }),
        ],
      ],
    ]);
    const range = computeTimeRange(messages);
    expect(range).toEqual({
      minMs: Date.parse('2026-01-01T00:00:00Z'),
      maxMs: Date.parse('2026-01-01T00:08:00Z'),
    });
  });

  it('returns null when every message lacks a usable timestamp', () => {
    const messages = new Map([
      [1, [message(1, 0, 'a', { createdAt: '' })]],
    ]);
    expect(computeTimeRange(messages)).toBeNull();
  });

  it('returns null for an empty map', () => {
    expect(computeTimeRange(new Map())).toBeNull();
  });
});

describe('xFraction', () => {
  it('maps the min/max bounds of a populated range to 0 and 1', () => {
    const range = { minMs: 0, maxMs: 1000 };
    expect(xFraction(0, range)).toBe(0);
    expect(xFraction(1000, range)).toBe(1);
    expect(xFraction(500, range)).toBe(0.5);
  });

  it('returns 0 for a degenerate range (single timestamp across all dots)', () => {
    const range = { minMs: 1000, maxMs: 1000 };
    expect(xFraction(1000, range)).toBe(0);
  });

  it('returns 0 when no range is known', () => {
    expect(xFraction(123, null)).toBe(0);
  });
});

describe('buildTimelineLanes', () => {
  it('builds one lane per thread, sorted oldest first', () => {
    const threads = [
      thread(2, {
        parent: 1,
        createdAt: '2026-01-01T00:05:00Z',
      }),
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
    ];
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes.map((l) => l.threadId)).toEqual([1, 2]);
  });

  it(`labels the main thread "${MAIN_LANE_LABEL}" regardless of the wire title`, () => {
    const threads = [
      thread(1, { title: 'a long session prompt the server stored here' }),
      thread(2, {
        parent: 1,
        title: 'branch one',
        createdAt: '2026-01-01T00:05:00Z',
      }),
    ];
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes[0]).toMatchObject({
      label: MAIN_LANE_LABEL,
      tooltip: MAIN_LANE_LABEL,
      isMain: true,
    });
    // Subthread label matches Navigator's source: the wire thread.title.
    expect(lanes[1]).toMatchObject({
      label: 'branch one',
      tooltip: 'branch one',
      isMain: false,
    });
  });

  it('falls back to `thread <id>` when a subthread title is empty', () => {
    const threads = [
      thread(1),
      thread(2, {
        parent: 1,
        title: '',
        createdAt: '2026-01-01T00:05:00Z',
      }),
    ];
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes[1].label).toBe('thread 2');
    expect(lanes[1].tooltip).toBe('thread 2');
  });

  it('places dots on the shared time axis as fractions of created_at across every lane', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        createdAt: '2026-01-01T00:01:00Z',
      }),
    ];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'c', { createdAt: '2026-01-01T00:04:00Z' }),
        ],
      ],
      [
        2,
        [
          message(2, 0, 'b', { createdAt: '2026-01-01T00:02:00Z' }),
          message(2, 1, 'd', { createdAt: '2026-01-01T00:08:00Z' }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    // x positions still derive purely from created_at; `kind` now annotates
    // each dot for the renderer's color choice (every message here is a
    // user-role line with no text blocks, so they classify as "other").
    expect(lanes[0].dots.map((d) => ({ uuid: d.uuid, x: d.x }))).toEqual([
      { uuid: 'a', x: 0 },
      { uuid: 'c', x: 0.5 },
    ]);
    expect(lanes[1].dots.map((d) => ({ uuid: d.uuid, x: d.x }))).toEqual([
      { uuid: 'b', x: 0.25 },
      { uuid: 'd', x: 1 },
    ]);
  });

  it('annotates each dot with its classifyMessage kind', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          // A genuine human turn (user role + text block) → "user".
          message(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          // An assistant reply → "other".
          message(1, 1, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hello' }],
            createdAt: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(lanes[0].dots.map((d) => ({ uuid: d.uuid, kind: d.kind }))).toEqual([
      { uuid: 'u', kind: 'user' },
      { uuid: 'a', kind: 'other' },
    ]);
  });

  it('collapses every dot to x=0 when all messages landed at the same instant', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '2026-01-01T00:00:00Z' }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(lanes[0].dots.map((d) => d.x)).toEqual([0, 0]);
  });

  it('drops messages without a usable timestamp from the lane (no guessing)', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '' }),
          message(1, 2, 'c', { createdAt: '2026-01-01T00:01:00Z' }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(lanes[0].dots.map((d) => d.uuid)).toEqual(['a', 'c']);
  });

  it('returns an empty `dots` array for a thread missing from the messages map', () => {
    const threads = [thread(1), thread(2, { parent: 1, rootUuid: 'uuid-a' })];
    const lanes = buildTimelineLanes(threads, new Map([[1, [message(1, 0)]]]));
    expect(lanes.map((l) => l.dots.length)).toEqual([1, 0]);
  });
});

describe('buildSortedMessages', () => {
  it('flattens every lane into one (created_at, seq) ascending list across lanes', () => {
    // Two lanes whose messages interleave in time — the wheel-driven step
    // navigation must walk through them in clock order, NOT lane order.
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        createdAt: '2026-01-01T00:01:00Z',
      }),
    ];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'c', { createdAt: '2026-01-01T00:04:00Z' }),
        ],
      ],
      [
        2,
        [
          message(2, 0, 'b', { createdAt: '2026-01-01T00:02:00Z' }),
          message(2, 1, 'd', { createdAt: '2026-01-01T00:08:00Z' }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    const sorted = buildSortedMessages(lanes);
    expect(sorted.map((s) => s.uuid)).toEqual(['a', 'b', 'c', 'd']);
    // Each entry carries the owning thread, so a cross-lane step knows
    // which subthread to switch to.
    expect(sorted.map((s) => s.threadId)).toEqual([1, 2, 1, 2]);
  });

  it('tie-breaks by ascending seq when two messages share the same created_at', () => {
    // Same instant across two messages: the tie-break must be seq, not
    // lane order or uuid — the transcript already orders by seq, and the
    // timeline must agree.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          // Out of seq order in the input to prove the sort fixes it.
          message(1, 5, 'late', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 2, 'early', { createdAt: '2026-01-01T00:00:00Z' }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    const sorted = buildSortedMessages(lanes);
    expect(sorted.map((s) => s.uuid)).toEqual(['early', 'late']);
    expect(sorted.map((s) => s.seq)).toEqual([2, 5]);
  });

  it('returns an empty list when no lane has any messages', () => {
    const threads = [thread(1), thread(2, { parent: 1 })];
    const sorted = buildSortedMessages(buildTimelineLanes(threads, new Map()));
    expect(sorted).toEqual([]);
  });
});

describe('findNearestMessageIndex', () => {
  it('returns the index of the message whose x is closest to the target', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        createdAt: '2026-01-01T00:01:00Z',
      }),
    ];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'c', { createdAt: '2026-01-01T00:04:00Z' }),
        ],
      ],
      [
        2,
        [
          message(2, 0, 'b', { createdAt: '2026-01-01T00:02:00Z' }),
        ],
      ],
    ]);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    // sorted is [a@0, b@0.5, c@1]
    expect(sorted[findNearestMessageIndex(sorted, 0.04)].uuid).toBe('a');
    expect(sorted[findNearestMessageIndex(sorted, 0.45)].uuid).toBe('b');
    expect(sorted[findNearestMessageIndex(sorted, 0.9)].uuid).toBe('c');
  });

  it('breaks click-distance ties by earlier timeMs first so the lookup is deterministic', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        createdAt: '2026-01-01T00:00:00Z',
      }),
    ];
    // Two messages equidistant from x=0.5 (a at 0, b at 1). The earlier
    // timeMs wins.
    const messagesByThread = new Map([
      [1, [message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' })]],
      [2, [message(2, 0, 'b', { createdAt: '2026-01-01T00:01:00Z' })]],
    ]);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    expect(sorted[findNearestMessageIndex(sorted, 0.5)].uuid).toBe('a');
  });

  it('returns -1 when the sorted list is empty', () => {
    expect(findNearestMessageIndex([], 0.5)).toBe(-1);
  });
});
