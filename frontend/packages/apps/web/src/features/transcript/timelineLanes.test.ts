import { describe, expect, it } from 'vitest';
import type { Message, Thread } from '@delta/wire-gen';
import {
  LANE_LABEL_PREFIX_LEN,
  MAIN_LANE_LABEL,
  buildTimelineLanes,
  laneLabelFromUuid,
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

function message(threadId: number, seq: number, uuid?: string): Message {
  return {
    uuid: uuid ?? `m-${threadId}-${seq}`,
    session_id: 'session-1',
    thread_id: threadId,
    role: 'user',
    linear_parent_uuid: null,
    semantic_parent_uuid: null,
    prompt_id: null,
    seq,
    content_text: null,
    content: [],
    created_at: '2026-01-01T00:00:00Z',
    model: null,
    git_branch: null,
    cwd: null,
    response_time_ms: null,
  };
}

describe('laneLabelFromUuid', () => {
  it(`returns at most ${LANE_LABEL_PREFIX_LEN} leading chars`, () => {
    const uuid = 'a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6';
    expect(laneLabelFromUuid(uuid)).toBe(uuid.slice(0, LANE_LABEL_PREFIX_LEN));
    expect(laneLabelFromUuid(uuid).length).toBe(LANE_LABEL_PREFIX_LEN);
  });

  it('returns the uuid unchanged when shorter than the prefix length', () => {
    expect(laneLabelFromUuid('short')).toBe('short');
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

  it(`labels the main thread "${MAIN_LANE_LABEL}" and subthreads by the root uuid prefix`, () => {
    const rootUuid = 'a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6';
    const threads = [
      thread(1),
      thread(2, {
        parent: 1,
        rootUuid,
        createdAt: '2026-01-01T00:05:00Z',
      }),
    ];
    const lanes = buildTimelineLanes(threads, new Map());
    expect(lanes[0]).toMatchObject({
      label: MAIN_LANE_LABEL,
      tooltip: MAIN_LANE_LABEL,
      isMain: true,
    });
    expect(lanes[1]).toMatchObject({
      label: rootUuid.slice(0, LANE_LABEL_PREFIX_LEN),
      tooltip: rootUuid,
      isMain: false,
    });
  });

  it('uses a `thread <id>` fallback when a subthread has no root uuid', () => {
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

  it('places dots in seq order with equal spacing indices 0..N-1', () => {
    const threads = [thread(1)];
    const messages = [message(1, 2, 'm3'), message(1, 0, 'm1'), message(1, 1, 'm2')];
    const lanes = buildTimelineLanes(threads, new Map([[1, messages]]));
    expect(lanes[0].dots).toEqual([
      { uuid: 'm1', threadId: 1, order: 0 },
      { uuid: 'm2', threadId: 1, order: 1 },
      { uuid: 'm3', threadId: 1, order: 2 },
    ]);
  });

  it('returns an empty `dots` array for a thread missing from the messages map', () => {
    const threads = [thread(1), thread(2, { parent: 1, rootUuid: 'uuid-a' })];
    const lanes = buildTimelineLanes(threads, new Map([[1, [message(1, 0)]]]));
    expect(lanes.map((l) => l.dots.length)).toEqual([1, 0]);
  });
});
