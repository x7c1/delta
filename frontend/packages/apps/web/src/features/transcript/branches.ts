import type { MessageUuid, Thread, ThreadId } from '@delta/model';

/**
 * Map each message uuid to the child threads that branch from it. A child
 * thread branches from `root_message_uuid`; chips render under that message.
 */
export function childThreadsByMessage(
  threads: Thread[],
  parentThreadId: ThreadId,
): Map<MessageUuid, Thread[]> {
  const map = new Map<MessageUuid, Thread[]>();
  for (const thread of threads) {
    if (
      thread.parent_thread_id === parentThreadId &&
      thread.root_message_uuid !== null
    ) {
      const list = map.get(thread.root_message_uuid) ?? [];
      list.push(thread);
      map.set(thread.root_message_uuid, list);
    }
  }
  return map;
}
