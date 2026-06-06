import type { MessageUuid } from './message';
import type { SessionId } from './session';
import type { ThreadId } from './thread';

/** Server-issued integer identifier for a pending send. */
export type PendingSendId = number;

export type PendingSendStatus = 'pending' | 'matched' | 'cancelled';

export interface PendingSend {
  id: PendingSendId;
  session_id: SessionId;
  thread_id: ThreadId;
  semantic_parent_uuid: MessageUuid | null;
  text: string;
  locator_quote: string | null;
  status: PendingSendStatus;
  matched_uuid: MessageUuid | null;
  created_at: string;
}
