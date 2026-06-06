import type { ContentBlock } from './content-block';
import type { SessionId } from './session';
import type { ThreadId } from './thread';

/** String identifier for a transcript message. */
export type MessageUuid = string;

/** String identifier for a prompt. */
export type PromptId = string;

export type MessageRole = 'user' | 'assistant' | 'system' | 'other';

export interface Message {
  uuid: MessageUuid;
  session_id: SessionId;
  thread_id: ThreadId;
  role: MessageRole;
  linear_parent_uuid: MessageUuid | null;
  semantic_parent_uuid: MessageUuid | null;
  prompt_id: PromptId | null;
  seq: number;
  content_text: string | null;
  content: ContentBlock[];
  created_at: string;
}
