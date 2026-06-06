import type { Message, MessageUuid } from './message';
import type { PendingSend } from './pending-send';
import type { Session } from './session';
import type { Thread, ThreadId } from './thread';

export interface SessionResponse {
  session: Session;
  main_thread_id: ThreadId;
}

export interface ThreadsResponse {
  threads: Thread[];
}

export interface MessagesResponse {
  messages: Message[];
}

export interface SendResponse {
  send: PendingSend;
}

/** Request body for `POST /api/sends`. */
export interface SendRequest {
  thread_id: ThreadId;
  text: string;
  locator_quote?: string;
  semantic_parent_uuid?: MessageUuid;
}
