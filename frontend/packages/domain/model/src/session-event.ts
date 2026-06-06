import type { MessageUuid } from './message';
import type { PendingSendId } from './pending-send';
import type { SessionId } from './session';

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

export type SessionEvent =
  | SessionRegisteredEvent
  | TurnStartedEvent
  | ExternalInputEvent
  | TurnCompletedEvent
  | PermissionRequestedEvent;

export type SessionEventKind = SessionEvent['kind'];
