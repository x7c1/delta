import type { MessageUuid } from './message';
import type { PendingSendId } from './pending-send';
import type { SessionId } from './session';
import type { ThreadId } from './thread';

export interface SessionRegisteredEvent {
  kind: 'session_registered';
  session_id: SessionId;
}

export interface TurnStartedEvent {
  kind: 'turn_started';
  session_id: SessionId;
  pending_send_id: PendingSendId;
  matched_uuid: MessageUuid;
}

export interface ExternalInputEvent {
  kind: 'external_input';
  session_id: SessionId;
  prompt: string;
}

export interface TurnCompletedEvent {
  kind: 'turn_completed';
  session_id: SessionId;
  stop_reason: string | null;
}

export interface PermissionRequestedEvent {
  kind: 'permission_requested';
  session_id: SessionId;
  request_id: number;
  tool_name: string;
}

/**
 * The transcript grew between hooks (continuous tail). Unlike `turn_completed`
 * and `external_input`, this carries no turn semantics: it must only refetch the
 * affected threads, never mutate the pending-send FIFO or unread badges.
 */
export interface TranscriptUpdatedEvent {
  kind: 'transcript_updated';
  session_id: SessionId;
  thread_ids: ThreadId[];
}

export type SessionEvent =
  | SessionRegisteredEvent
  | TurnStartedEvent
  | ExternalInputEvent
  | TurnCompletedEvent
  | PermissionRequestedEvent
  | TranscriptUpdatedEvent;

export type SessionEventKind = SessionEvent['kind'];
