import type { Thread, ThreadId } from './thread';

/**
 * A node in the thread navigator tree. Derived from the flat `Thread[]` list;
 * children are ordered by creation (ascending id), matching the server order.
 */
export interface ThreadNode {
  thread: Thread;
  children: ThreadNode[];
}

/**
 * Build a forest of {@link ThreadNode}s from the flat thread list. Roots are
 * threads with no parent (or whose parent is absent). Siblings preserve the
 * input order, which the server guarantees is ascending creation order.
 */
export function buildThreadTree(threads: Thread[]): ThreadNode[] {
  const nodes = new Map<ThreadId, ThreadNode>();
  for (const thread of threads) {
    nodes.set(thread.id, { thread, children: [] });
  }
  const roots: ThreadNode[] = [];
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
 * Walk from a thread up to the root, returning the ancestor chain ordered
 * root-first (so the last element is the thread itself). Used to render the
 * transcript breadcrumb. Threads whose parent is missing terminate the walk.
 */
export function threadAncestry(
  threads: Thread[],
  threadId: ThreadId,
): Thread[] {
  const byId = new Map<ThreadId, Thread>();
  for (const thread of threads) {
    byId.set(thread.id, thread);
  }
  const chain: Thread[] = [];
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
