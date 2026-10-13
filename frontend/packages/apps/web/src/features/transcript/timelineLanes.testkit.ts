/**
 * Shared thread and message builders for the `timelineLanes.*.test.ts` family.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import type { Message, Thread } from '@delta/wire-gen';

export function thread(
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
    last_activity_at: null,
    created_at: createdAt,
  };
}

export function message(
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
    provider_item_id: null,
  };
}
