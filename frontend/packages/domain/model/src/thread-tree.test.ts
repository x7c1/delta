import { describe, expect, it } from 'vitest';
import {
  buildThreadTree,
  newestThreadId,
  threadAncestry,
  type ThreadLike,
} from './thread-tree';

/**
 * A wire-`Thread`-shaped fixture. The helpers are generic over {@link
 * ThreadLike}, so the test mirrors how callers pass the richer wire record and
 * get it back out (`title` rides along untouched).
 */
interface TestThread extends ThreadLike {
  title: string;
}

function thread(id: number, parent: number | null): TestThread {
  return {
    id,
    title: id === 1 ? 'main' : `thread-${id}`,
    parent_thread_id: parent,
  };
}

describe('buildThreadTree', () => {
  it('nests children under their parent in creation order', () => {
    const threads = [
      thread(1, null),
      thread(2, 1),
      thread(3, 1),
      thread(4, 2),
    ];

    const roots = buildThreadTree(threads);

    expect(roots).toHaveLength(1);
    const main = roots[0];
    expect(main.thread.id).toBe(1);
    expect(main.children.map((c) => c.thread.id)).toEqual([2, 3]);
    expect(main.children[0].children.map((c) => c.thread.id)).toEqual([4]);
  });

  it('returns an empty forest for no threads', () => {
    expect(buildThreadTree([])).toEqual([]);
  });

  it('treats a thread with a missing parent as a root', () => {
    const roots = buildThreadTree([thread(5, 99)]);
    expect(roots.map((r) => r.thread.id)).toEqual([5]);
  });
});

describe('threadAncestry', () => {
  it('returns the root-first chain to the target thread', () => {
    const threads = [thread(1, null), thread(2, 1), thread(4, 2)];

    expect(threadAncestry(threads, 4).map((t) => t.id)).toEqual([1, 2, 4]);
  });

  it('returns just the thread itself for a root', () => {
    expect(threadAncestry([thread(1, null)], 1).map((t) => t.id)).toEqual([1]);
  });
});

describe('newestThreadId', () => {
  const at = (id: number, last_activity_at: string | null) => ({
    id,
    last_activity_at,
  });

  it('returns the id of the thread with the greatest activity timestamp', () => {
    expect(
      newestThreadId([
        at(1, '2026-01-01T00:00:00Z'),
        at(2, '2026-01-01T00:09:00Z'),
        at(3, '2026-01-01T00:05:00Z'),
      ]),
    ).toBe(2);
  });

  it('ignores threads with no activity', () => {
    expect(
      newestThreadId([at(1, null), at(2, '2026-01-01T00:01:00Z'), at(3, null)]),
    ).toBe(2);
  });

  it('breaks a tie on the larger id so exactly one thread wins', () => {
    expect(
      newestThreadId([
        at(3, '2026-01-01T00:04:00Z'),
        at(7, '2026-01-01T00:04:00Z'),
        at(5, '2026-01-01T00:04:00Z'),
      ]),
    ).toBe(7);
  });

  it('returns undefined when no thread has any activity', () => {
    expect(newestThreadId([at(1, null), at(2, null)])).toBeUndefined();
    expect(newestThreadId([])).toBeUndefined();
  });
});
