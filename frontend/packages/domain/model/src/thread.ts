import type { MessageUuid } from './message';
import type { SessionId } from './session';

/** Server-issued integer identifier for a thread. */
export type ThreadId = number;

export interface Thread {
  id: ThreadId;
  session_id: SessionId;
  title: string;
  parent_thread_id: ThreadId | null;
  root_message_uuid: MessageUuid | null;
  created_at: string;
}
