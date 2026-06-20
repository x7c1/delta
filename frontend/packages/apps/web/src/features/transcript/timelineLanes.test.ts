import { describe, expect, it } from 'vitest';
import type { Message, Thread } from '@delta/wire-gen';
import {
  LANE_LABEL_PREFIX_LEN,
  MAIN_LANE_LABEL,
  NO_PREVIEW_LANE_LABEL,
  buildTimelineLanes,
  computeTimeRange,
  findActiveMessage,
  laneLabelFromText,
  messagePreviewText,
  messageTimeMs,
  xFraction,
} from './timelineLanes';

function thread(
  id: number,
  {
    parent = null,
    rootUuid = null,
    createdAt = '2026-01-01T00:00:00Z',
  }: {
    parent?: number | null;
    rootUuid?: string | null;
    createdAt?: string;
  } = {},
): Thread {
  return {
    id,
    session_id: 'session-1',
    title: `thread ${id}`,
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
  } = {},
): Message {
  return {
    uuid: uuid ?? `m-${threadId}-${seq}`,
    session_id: 'session-1',
    thread_id: threadId,
    role: 'user',
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

describe('laneLabelFromText', () => {
  it(`returns at most ${LANE_LABEL_PREFIX_LEN} leading chars`, () => {
    const long = 'a'.repeat(50);
    expect(laneLabelFromText(long)).toBe(long.slice(0, LANE_LABEL_PREFIX_LEN));
    expect(laneLabelFromText(long).length).toBe(LANE_LABEL_PREFIX_LEN);
  });

  it('returns the text unchanged when shorter than the prefix length', () => {
    expect(laneLabelFromText('short')).toBe('short');
  });
});

describe('messagePreviewText', () => {
  it('prefers content_text when present', () => {
    const m = message(1, 0, 'm1', { contentText: 'hello world' });
    expect(messagePreviewText(m)).toBe('hello world');
  });

  it('falls back to joining `text` content blocks when content_text is null', () => {
    const m = message(1, 0, 'm1', {
      contentText: null,
      content: [
        { type: 'text', text: 'first ' },
        { type: 'thinking', thinking: 'private' },
        { type: 'text', text: 'second' },
      ],
    });
    expect(messagePreviewText(m)).toBe('first second');
  });

  it('collapses newlines and runs of whitespace into single spaces', () => {
    const m = message(1, 0, 'm1', {
      contentText: '  line one\n\n  line  two\t\tend  ',
    });
    expect(messagePreviewText(m)).toBe('line one line two end');
  });

  it('returns empty string when there is no visible text', () => {
    const m = message(1, 0, 'm1', { contentText: null, content: [] });
    expect(messagePreviewText(m)).toBe('');
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
        rootUuid: 'uuid-a',
        createdAt: '2026-01-01T00:05:00Z',
      }),
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
    ];
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes.map((l) => l.threadId)).toEqual([1, 2]);
  });

  it(`labels the main thread "${MAIN_LANE_LABEL}" and subthreads by the root message body prefix`, () => {
    const rootUuid = 'root-uuid-1';
    const rootBody = 'Plan the next migration step in detail';
    const threads = [
      thread(1),
      thread(2, {
        parent: 1,
        rootUuid,
        createdAt: '2026-01-01T00:05:00Z',
      }),
    ];
    // The root message lives in the parent thread's message list — that is
    // exactly the cross-lane lookup the builder has to perform.
    const messagesByThread = new Map([
      [1, [message(1, 0, rootUuid, { contentText: rootBody })]],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(lanes[0]).toMatchObject({
      label: MAIN_LANE_LABEL,
      tooltip: MAIN_LANE_LABEL,
      isMain: true,
    });
    expect(lanes[1]).toMatchObject({
      label: rootBody.slice(0, LANE_LABEL_PREFIX_LEN),
      tooltip: rootBody,
      isMain: false,
    });
  });

  it(`falls back to "${NO_PREVIEW_LANE_LABEL}" with the root uuid as tooltip when the root message is not loaded yet`, () => {
    const rootUuid = 'root-uuid-1';
    const threads = [
      thread(1),
      thread(2, {
        parent: 1,
        rootUuid,
        createdAt: '2026-01-01T00:05:00Z',
      }),
    ];
    // No message map entry for the root — the per-thread fetch is in flight.
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes[1].label).toBe(NO_PREVIEW_LANE_LABEL);
    expect(lanes[1].tooltip).toBe(rootUuid);
  });

  it('uses a `thread <id>` fallback when a subthread has no root uuid at all', () => {
    const threads = [
      thread(1),
      thread(2, {
        parent: 1,
        rootUuid: null,
        createdAt: '2026-01-01T00:05:00Z',
      }),
    ];
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes[1].label).toBe('thread 2');
    expect(lanes[1].tooltip).toBe('thread 2');
  });

  it('places dots on the shared time axis as fractions of created_at across every lane', () => {
    // Two interleaving lanes spanning 8 minutes. The dot fractions must be
    // proportional to the message timestamps, NOT to the per-lane index:
    // earliest at 0, latest at 1, mid-span dots in between.
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        rootUuid: null,
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
    expect(lanes[0].dots).toEqual([
      {
        uuid: 'a',
        threadId: 1,
        x: 0,
        timeMs: Date.parse('2026-01-01T00:00:00Z'),
      },
      {
        uuid: 'c',
        threadId: 1,
        x: 0.5,
        timeMs: Date.parse('2026-01-01T00:04:00Z'),
      },
    ]);
    expect(lanes[1].dots).toEqual([
      {
        uuid: 'b',
        threadId: 2,
        x: 0.25,
        timeMs: Date.parse('2026-01-01T00:02:00Z'),
      },
      {
        uuid: 'd',
        threadId: 2,
        x: 1,
        timeMs: Date.parse('2026-01-01T00:08:00Z'),
      },
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

describe('findActiveMessage', () => {
  it('returns the dot whose x is closest to the playhead across all lanes', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        rootUuid: null,
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
    const lanes = buildTimelineLanes(threads, messagesByThread);
    // Dots: a=0, b=0.5, c=1
    expect(findActiveMessage(lanes, 0.04)?.uuid).toBe('a');
    expect(findActiveMessage(lanes, 0.45)?.uuid).toBe('b');
    expect(findActiveMessage(lanes, 0.9)?.uuid).toBe('c');
  });

  it('breaks ties by earlier timeMs first so the lookup is deterministic', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        rootUuid: null,
        createdAt: '2026-01-01T00:00:00Z',
      }),
    ];
    // Two dots equidistant from x=0.5 (a at 0, b at 1). The earlier timeMs wins.
    const messagesByThread = new Map([
      [1, [message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' })]],
      [2, [message(2, 0, 'b', { createdAt: '2026-01-01T00:01:00Z' })]],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(findActiveMessage(lanes, 0.5)?.uuid).toBe('a');
  });

  it('returns null when no lane has any dots', () => {
    const threads = [thread(1)];
    expect(findActiveMessage(buildTimelineLanes(threads, new Map()), 0.5)).toBeNull();
  });
});
