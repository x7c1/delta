import type { ThreadId } from './ids';

/**
 * The minimal structural shape the thread-tree helpers need.
 *
 * The full thread record lives on the wire (`Thread` in @delta/wire-gen);
 * this package is dependency-free, so the helpers are generic over anything
 * carrying the tree edges. Callers pass the wire `Thread` and get its full
 * shape back out.
 */
export interface ThreadLike {
  id: ThreadId;
  parent_thread_id: ThreadId | null;
}

/**
 * A node in the thread navigator tree. Derived from the flat thread list;
 * children are ordered by creation (ascending id), matching the server order.
 */
export interface ThreadNode<T extends ThreadLike> {
  thread: T;
  children: ThreadNode<T>[];
}

/**
 * Build a forest of {@link ThreadNode}s from the flat thread list. Roots are
 * threads with no parent (or whose parent is absent). Siblings preserve the
 * input order, which the server guarantees is ascending creation order.
 */
export function buildThreadTree<T extends ThreadLike>(
  threads: T[],
): ThreadNode<T>[] {
  const nodes = new Map<ThreadId, ThreadNode<T>>();
  for (const thread of threads) {
    nodes.set(thread.id, { thread, children: [] });
  }
  const roots: ThreadNode<T>[] = [];
  for (const thread of threads) {
    const node = nodes.get(thread.id)!;
    const parentId = thread.parent_thread_id;
    const parent = parentId === null ? undefined : nodes.get(parentId);
    if (parent) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

/**
 * The minimal shape {@link newestThreadId} ranks: an id and the thread's last
 * activity. Kept separate from {@link ThreadLike} because recency has nothing
 * to do with the tree edges — a caller ranking threads need not have them.
 */
export interface ThreadActivityLike {
  id: ThreadId;
  last_activity_at: string | null;
}

/**
 * The id of the thread whose activity is newest, or `undefined` when no thread
 * has any. Timestamps are compared as strings: they are ISO-8601 UTC in one
 * format, so lexicographic order is chronological order.
 *
 * A tie breaks on the larger id, so the answer is always exactly one thread —
 * the navigator marks a single row, and "two threads at the same second" must
 * not mark both.
 *
 * Callers pass EVERY thread of the session, main included. The navigator does
 * not render main, so naming it here is what makes "no row is marked" mean
 * "main is where the last message landed".
 */
export function newestThreadId(
  threads: ThreadActivityLike[],
): ThreadId | undefined {
  let newest: { id: ThreadId; at: string } | undefined;
  for (const thread of threads) {
    const at = thread.last_activity_at;
    if (at === null) {
      continue;
    }
    if (
      newest === undefined ||
      at > newest.at ||
      (at === newest.at && thread.id > newest.id)
    ) {
      newest = { id: thread.id, at };
    }
  }
  return newest?.id;
}

/**
 * Walk from a thread up to the root, returning the ancestor chain ordered
 * root-first (so the last element is the thread itself). Used to render the
 * transcript breadcrumb. Threads whose parent is missing terminate the walk.
 */
export function threadAncestry<T extends ThreadLike>(
  threads: T[],
  threadId: ThreadId,
): T[] {
  const byId = new Map<ThreadId, T>();
  for (const thread of threads) {
    byId.set(thread.id, thread);
  }
  const chain: T[] = [];
  let current = byId.get(threadId);
  const seen = new Set<ThreadId>();
  while (current && !seen.has(current.id)) {
    seen.add(current.id);
    chain.push(current);
    current =
      current.parent_thread_id === null
        ? undefined
        : byId.get(current.parent_thread_id);
  }
  return chain.reverse();
}
