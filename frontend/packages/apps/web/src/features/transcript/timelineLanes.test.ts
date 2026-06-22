import { describe, expect, it } from 'vitest';
import type { Message, Thread } from '@delta/wire-gen';
import {
  MAIN_LANE_LABEL,
  buildGlobalXMap,
  buildLaneRenderItems,
  buildLargeSortedMessages,
  buildSortedMessages,
  buildTimelineLanes,
  classifyMessage,
  classifyMessageSize,
  computeTimeRange,
  findNearestMessageIndex,
  messageBelongsOnTimeline,
  messageTimeMs,
  xFraction,
  type TimelineDot,
  type TimelineDotSize,
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

  it('classifies a task-notification user turn as "other"', () => {
    // The harness submits the notification as a normal user-role line with a
    // `<task-notification>` prefix. It is not human prose, so the timeline
    // mark must not paint it as a headline user turn.
    const m = message(1, 0, 'tn', {
      role: 'user',
      content: [{ type: 'text', text: '<task-notification>foo</task-notification>' }],
    });
    expect(classifyMessage(m)).toBe('other');
  });
});

describe('classifyMessageSize', () => {
  it('classifies a user-role message with author text as "large"', () => {
    const m = message(1, 0, 'm', {
      role: 'user',
      content: [{ type: 'text', text: 'hi' }],
    });
    expect(classifyMessageSize(m)).toBe('large');
  });

  it('classifies an assistant-role message with prose text as "large"', () => {
    const m = message(1, 0, 'a', {
      role: 'assistant',
      content: [{ type: 'text', text: 'hello' }],
    });
    expect(classifyMessageSize(m)).toBe('large');
  });

  it('classifies an assistant tool-only call as "small"', () => {
    // Pure tool_use, no prose text → small. The transcript renders this as
    // a collapsible card, not a bubble, so the timeline mirrors that split.
    const m = message(1, 0, 't', {
      role: 'assistant',
      content: [{ type: 'tool_use', id: 'tu', name: 'Bash', input: {} }],
    });
    expect(classifyMessageSize(m)).toBe('small');
  });

  it('classifies a user-role tool-result carrier as "small"', () => {
    // No author text, just a tool_result block — the transcript renders
    // this on the assistant side; the timeline mirrors with `small`.
    const m = message(1, 0, 'r', {
      role: 'user',
      content: [
        { type: 'tool_result', tool_use_id: 'tu', content: '', is_error: false },
      ],
    });
    expect(classifyMessageSize(m)).toBe('small');
  });

  it('classifies meta and system rows as "small"', () => {
    expect(
      classifyMessageSize(
        message(1, 0, 'm', { role: 'meta', content: [{ type: 'text', text: 'x' }] }),
      ),
    ).toBe('small');
    expect(
      classifyMessageSize(
        message(1, 0, 's', { role: 'system', content: [{ type: 'text', text: 'x' }] }),
      ),
    ).toBe('small');
  });

  it('classifies a task-notification user turn as "small"', () => {
    // The harness wraps the notification in a `text` block, which would
    // otherwise count as a "main" turn. Detect it explicitly so the timeline
    // mark stays auxiliary alongside the surrounding tool calls.
    const m = message(1, 0, 'tn', {
      role: 'user',
      content: [
        { type: 'text', text: '<task-notification>foo</task-notification>' },
      ],
    });
    expect(classifyMessageSize(m)).toBe('small');
  });
});

describe('buildLargeSortedMessages', () => {
  it('returns only the main-conversation subset in the same (timeMs, seq) order', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          message(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            createdAt: '2026-01-01T00:01:00Z',
          }),
          message(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'reply' }],
            createdAt: '2026-01-01T00:02:00Z',
          }),
          message(1, 3, 'm', {
            role: 'meta',
            content: [{ type: 'text', text: 'sys' }],
            createdAt: '2026-01-01T00:03:00Z',
          }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    const large = buildLargeSortedMessages(lanes);
    expect(large.map((m) => m.uuid)).toEqual(['u', 'a']);
    expect(large.every((m) => m.size === 'large')).toBe(true);
  });

  it('returns an empty list when no lane has any large messages', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
          }),
        ],
      ],
    ]);
    const large = buildLargeSortedMessages(
      buildTimelineLanes(threads, messagesByThread),
    );
    expect(large).toEqual([]);
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
  it('returns the index of the message whose px x is closest to the target', () => {
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
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    // Project onto a 240 px axis. With ideals 0, 120, 240 px and the
    // greedy spacing push, the actuals stay at 0, 120, 240 (no collisions).
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, () => 6, 240);
    expect(sorted[findNearestMessageIndex(sorted, pxByUuid, 10)].uuid).toBe('a');
    expect(sorted[findNearestMessageIndex(sorted, pxByUuid, 110)].uuid).toBe('b');
    expect(sorted[findNearestMessageIndex(sorted, pxByUuid, 220)].uuid).toBe('c');
  });

  it('breaks click-distance ties by earlier timeMs first so the lookup is deterministic', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        createdAt: '2026-01-01T00:00:00Z',
      }),
    ];
    // Two messages equidistant from x=120 (a at 0, b at 240). The earlier
    // timeMs wins.
    const messagesByThread = new Map([
      [1, [message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' })]],
      [2, [message(2, 0, 'b', { createdAt: '2026-01-01T00:01:00Z' })]],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, () => 6, 240);
    expect(sorted[findNearestMessageIndex(sorted, pxByUuid, 120)].uuid).toBe('a');
  });

  it('returns -1 when the sorted list is empty', () => {
    expect(findNearestMessageIndex([], new Map(), 0)).toBe(-1);
  });
});

describe('buildGlobalXMap', () => {
  // Real renderer values; tests pin the contract those constants drive.
  const MARK_LARGE_PX = 6;
  const MARK_SMALL_PX = 4;
  const diameter = (size: TimelineDotSize) =>
    size === 'large' ? MARK_LARGE_PX : MARK_SMALL_PX;

  it('places a message at the same px x in every lane it appears next to', () => {
    // Lane 1 carries msg-a (early) and msg-c (late). Lane 2 carries msg-b
    // (mid) at the same instant a hypothetical lane-1 message would land —
    // the test enforces that the global map collapses both ideal positions
    // to ONE px value keyed by uuid, so cross-lane rendering lines up.
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, { parent: 1, createdAt: '2026-01-01T00:01:00Z' }),
    ];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          // c shares msg-b's timestamp — they collide ideally. The greedy
          // push moves the later one (by seq) right by the min spacing, so
          // both lanes still see the SAME map entries for both uuids.
          message(1, 5, 'c', {
            role: 'user',
            content: [{ type: 'text', text: 'late' }],
            createdAt: '2026-01-01T00:02:00Z',
          }),
        ],
      ],
      [
        2,
        [
          message(2, 1, 'b', {
            role: 'user',
            content: [{ type: 'text', text: 'mid' }],
            createdAt: '2026-01-01T00:02:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    // a at ts 0 → ideal 0 px → actual 0 px (first message keeps its ideal).
    expect(pxByUuid.get('a')).toBe(0);
    // b at ts 120s of 120s range → ideal 240 px → first to land at that
    // timestamp, so actual = 240 px.
    expect(pxByUuid.get('b')).toBe(240);
    // c shares ts with b → ideal 240 px, but b already occupies 240. The
    // greedy push moves c to 240 + (6+6)/2 = 246 px.
    expect(pxByUuid.get('c')).toBe(246);
  });

  it('enforces minimum spacing so overlapping ideal positions get pushed apart', () => {
    // Three large messages within a 1 ms window: their ideal positions
    // collapse to ~0 px on a 240 px axis, so the greedy push must spread
    // them out by 6 px each (large+large min spacing).
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', {
            role: 'user',
            content: [{ type: 'text', text: 'a' }],
            createdAt: '2026-01-01T00:00:00.000Z',
          }),
          message(1, 1, 'b', {
            role: 'user',
            content: [{ type: 'text', text: 'b' }],
            createdAt: '2026-01-01T00:00:00.001Z',
          }),
          message(1, 2, 'c', {
            role: 'user',
            content: [{ type: 'text', text: 'c' }],
            createdAt: '2026-01-01T00:00:00.002Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid, axisWidth } = buildGlobalXMap(
      sorted,
      range,
      240,
      diameter,
      240,
    );
    expect(pxByUuid.get('a')).toBe(0);
    // b's ideal is ~0.005 px (1 ms out of 2 ms range × 240 px = 120 px,
    // actually, let me recompute: 1/2 × 240 = 120 px). Not overlapping,
    // stays at 120.
    expect(pxByUuid.get('b')).toBe(120);
    expect(pxByUuid.get('c')).toBe(240);
    expect(axisWidth).toBe(240);
  });

  it('enforces large+small min spacing of 5 px between mixed neighbours', () => {
    // A large then a small, both at the same timestamp → ideals collide at
    // 0 px. The push moves the small to (6+4)/2 = 5 px.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'L', {
            role: 'user',
            content: [{ type: 'text', text: 'large' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          // Same instant, classified small (tool_use only).
          message(1, 1, 's', {
            role: 'assistant',
            content: [{ type: 'tool_use', id: 'tu', name: 'Bash', input: {} }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    expect(pxByUuid.get('L')).toBe(0);
    expect(pxByUuid.get('s')).toBe(5);
  });

  it('enforces small+small min spacing of 4 px between two auxiliary neighbours', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 's1', {
            role: 'assistant',
            content: [{ type: 'tool_use', id: 'tu1', name: 'Bash', input: {} }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          message(1, 1, 's2', {
            role: 'assistant',
            content: [{ type: 'tool_use', id: 'tu2', name: 'Bash', input: {} }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    expect(pxByUuid.get('s1')).toBe(0);
    expect(pxByUuid.get('s2')).toBe(4);
  });

  it('expands axisWidth past the minimum when overlap pushes the last mark right', () => {
    // Four messages, every one at the rightmost timestamp. The first lands
    // at the ideal max (240 px); the next three get pushed right by 6 px
    // each, so axisWidth grows past the minimum.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '2026-01-01T00:01:00Z' }),
          message(1, 2, 'c', { createdAt: '2026-01-01T00:01:00Z' }),
          message(1, 3, 'd', { createdAt: '2026-01-01T00:01:00Z' }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    // Every message here is a user-role no-content → small (4 px). Spacing
    // between two smalls is 4 px.
    const { pxByUuid, axisWidth } = buildGlobalXMap(
      sorted,
      range,
      240,
      diameter,
      240,
    );
    expect(pxByUuid.get('a')).toBe(0);
    expect(pxByUuid.get('b')).toBe(240);
    expect(pxByUuid.get('c')).toBe(244);
    expect(pxByUuid.get('d')).toBe(248);
    expect(axisWidth).toBe(248);
  });

  it('keeps axisWidth at the minimum when no message exists yet', () => {
    const { pxByUuid, axisWidth } = buildGlobalXMap([], null, 240, diameter, 240);
    expect(pxByUuid.size).toBe(0);
    expect(axisWidth).toBe(240);
  });
});

describe('messageBelongsOnTimeline', () => {
  it('keeps user / assistant / meta messages on the timeline', () => {
    for (const role of ['user', 'assistant', 'meta'] as Message['role'][]) {
      expect(
        messageBelongsOnTimeline(
          message(1, 0, 'm', { role, content: [{ type: 'text', text: 'x' }] }),
        ),
      ).toBe(true);
    }
  });

  it('drops system and other rows — they are ingest-only', () => {
    // These rows are recorded for parser fidelity but never rendered in
    // the transcript; including them in the timeline surfaced as "mystery"
    // small dots to the LEFT of the first user message because their
    // created_at often precedes the first prompt.
    for (const role of ['system', 'other'] as Message['role'][]) {
      expect(
        messageBelongsOnTimeline(
          message(1, 0, 'm', { role, content: [{ type: 'text', text: 'x' }] }),
        ),
      ).toBe(false);
    }
  });
});

describe('buildTimelineLanes role filter', () => {
  it('excludes system and other rows from lane dots so a bootstrap stamp does not surface', () => {
    // The bootstrap row's created_at is earlier than the first user prompt.
    // Without the role filter it would surface as a small dot to the left of
    // the user's first message — the v6 "mystery dot" symptom on real
    // hardware. The filter must drop it without touching any user-readable
    // message.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'sys-boot', {
            role: 'system',
            content: [{ type: 'text', text: 'session bootstrap' }],
            createdAt: '2025-12-31T23:59:00Z',
          }),
          message(1, 1, 'user-first', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          message(1, 2, 'other-misc', {
            role: 'other',
            content: [{ type: 'text', text: 'misc' }],
            createdAt: '2026-01-01T00:00:30Z',
          }),
          message(1, 3, 'assistant-reply', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            createdAt: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(lanes[0].dots.map((d) => d.uuid)).toEqual([
      'user-first',
      'assistant-reply',
    ]);
  });

  it('computes the time range from the filtered set so a bootstrap row does not stretch the axis left', () => {
    // The bootstrap row's earlier stamp must not pull `minMs` back — that
    // would shift every visible dot to the right of the axis origin and
    // re-introduce a wide gap at the left edge where the dropped dot used
    // to sit.
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'sys-boot', {
            role: 'system',
            createdAt: '2025-12-31T23:00:00Z',
          }),
          message(1, 1, 'user-first', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          message(1, 2, 'assistant-reply', {
            role: 'assistant',
            content: [{ type: 'text', text: 'reply' }],
            createdAt: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    expect(range).toEqual({
      minMs: Date.parse('2026-01-01T00:00:00Z'),
      maxMs: Date.parse('2026-01-01T00:01:00Z'),
    });
  });
});

describe('buildLaneRenderItems', () => {
  function dot(
    uuid: string,
    size: TimelineDotSize,
    overrides: Partial<TimelineDot> = {},
  ): TimelineDot {
    return {
      uuid,
      threadId: 1,
      x: 0,
      timeMs: 0,
      seq: 0,
      kind: 'other',
      size,
      ...overrides,
    };
  }

  it('returns single-dot items when every dot is large (no clustering needed)', () => {
    const items = buildLaneRenderItems([
      dot('a', 'large'),
      dot('b', 'large'),
      dot('c', 'large'),
    ]);
    expect(items.map((i) => i.kind)).toEqual(['dot', 'dot', 'dot']);
    expect(items).toHaveLength(3);
  });

  it('keeps a lone small dot as a single dot (clustering needs 2+)', () => {
    const items = buildLaneRenderItems([
      dot('L1', 'large'),
      dot('s', 'small'),
      dot('L2', 'large'),
    ]);
    expect(items.map((i) => i.kind)).toEqual(['dot', 'dot', 'dot']);
  });

  it('collapses 2+ consecutive small dots into one cluster pointing at the first', () => {
    const items = buildLaneRenderItems([
      dot('L1', 'large'),
      dot('s1', 'small'),
      dot('s2', 'small'),
      dot('s3', 'small'),
      dot('L2', 'large'),
    ]);
    expect(items).toHaveLength(3);
    expect(items[0].kind).toBe('dot');
    expect(items[2].kind).toBe('dot');
    expect(items[1].kind).toBe('cluster');
    if (items[1].kind === 'cluster') {
      expect(items[1].cluster.representativeUuid).toBe('s1');
      expect(items[1].cluster.memberCount).toBe(3);
      expect(items[1].cluster.memberUuids).toEqual(['s1', 's2', 's3']);
    }
  });

  it('does not cross a large dot when clustering — each run ends at the boundary', () => {
    // Two small runs separated by a large dot. Each run must cluster on
    // its own side; the cluster must not span the large dot in the middle.
    const items = buildLaneRenderItems([
      dot('s1', 'small'),
      dot('s2', 'small'),
      dot('L', 'large'),
      dot('s3', 'small'),
      dot('s4', 'small'),
    ]);
    expect(items.map((i) => i.kind)).toEqual(['cluster', 'dot', 'cluster']);
    if (items[0].kind === 'cluster' && items[2].kind === 'cluster') {
      expect(items[0].cluster.memberUuids).toEqual(['s1', 's2']);
      expect(items[2].cluster.memberUuids).toEqual(['s3', 's4']);
    }
  });

  it('clusters a small run that runs to the end of the lane', () => {
    const items = buildLaneRenderItems([
      dot('L', 'large'),
      dot('s1', 'small'),
      dot('s2', 'small'),
    ]);
    expect(items.map((i) => i.kind)).toEqual(['dot', 'cluster']);
    if (items[1].kind === 'cluster') {
      expect(items[1].cluster.memberUuids).toEqual(['s1', 's2']);
    }
  });

  it('clusters a small run that starts at the very beginning of the lane', () => {
    const items = buildLaneRenderItems([
      dot('s1', 'small'),
      dot('s2', 'small'),
      dot('L', 'large'),
    ]);
    expect(items.map((i) => i.kind)).toEqual(['cluster', 'dot']);
    if (items[0].kind === 'cluster') {
      expect(items[0].cluster.memberUuids).toEqual(['s1', 's2']);
    }
  });

  it('returns no items for an empty lane', () => {
    expect(buildLaneRenderItems([])).toEqual([]);
  });
});
