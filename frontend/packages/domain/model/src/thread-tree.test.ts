import { describe, expect, it } from 'vitest';
import { buildThreadTree, threadAncestry, type ThreadLike } from './thread-tree';

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
