import { describe, expect, it } from 'vitest';
import type { Message, Thread } from '@delta/wire-gen';
import {
  LANE_LABEL_PREFIX_LEN,
  MAIN_LANE_LABEL,
  NO_PREVIEW_LANE_LABEL,
  buildTimelineLanes,
  laneLabelFromText,
  messagePreviewText,
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

  it('places dots on the shared global axis sorted across lanes by `created_at`', () => {
    // Two lanes that interleave in wall-clock time. The dot `order` values
    // must reflect the merged chronological position, not the per-lane index:
    // m1-0 -> 0, m2-0 -> 1, m1-1 -> 2, m2-1 -> 3.
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
          message(1, 1, 'c', { createdAt: '2026-01-01T00:02:00Z' }),
        ],
      ],
      [
        2,
        [
          message(2, 0, 'b', { createdAt: '2026-01-01T00:01:00Z' }),
          message(2, 1, 'd', { createdAt: '2026-01-01T00:03:00Z' }),
        ],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    expect(lanes[0].dots).toEqual([
      { uuid: 'a', threadId: 1, order: 0 },
      { uuid: 'c', threadId: 1, order: 2 },
    ]);
    expect(lanes[1].dots).toEqual([
      { uuid: 'b', threadId: 2, order: 1 },
      { uuid: 'd', threadId: 2, order: 3 },
    ]);
  });

  it('breaks `created_at` ties by `seq` so global order is total and stable', () => {
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, {
        parent: 1,
        rootUuid: null,
        createdAt: '2026-01-01T00:00:00Z',
      }),
    ];
    // Same created_at second across two lanes — `seq` (per-session monotonic)
    // breaks the tie so the merged order is m1-0, m2-1, m1-2.
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 2, 'c', { createdAt: '2026-01-01T00:00:00Z' }),
        ],
      ],
      [
        2,
        [message(2, 1, 'b', { createdAt: '2026-01-01T00:00:00Z' })],
      ],
    ]);
    const lanes = buildTimelineLanes(threads, messagesByThread);
    const orderByUuid = new Map<string, number>();
    for (const lane of lanes) {
      for (const dot of lane.dots) {
        orderByUuid.set(dot.uuid, dot.order);
      }
    }
    expect(orderByUuid.get('a')).toBe(0);
    expect(orderByUuid.get('b')).toBe(1);
    expect(orderByUuid.get('c')).toBe(2);
  });

  it('returns an empty `dots` array for a thread missing from the messages map', () => {
    const threads = [thread(1), thread(2, { parent: 1, rootUuid: 'uuid-a' })];
    const lanes = buildTimelineLanes(threads, new Map([[1, [message(1, 0)]]]));
    expect(lanes.map((l) => l.dots.length)).toEqual([1, 0]);
  });
});
