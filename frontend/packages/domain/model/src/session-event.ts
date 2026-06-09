import type { MessageUuid } from './message';
import type { PendingSendId } from './pending-send';
import type { SessionId } from './session';
import type { ThreadId } from './thread';

export interface SessionRegisteredEvent {
  kind: 'session_registered';
  session_id: SessionId;
}

/** A closed session was resumed (or a new session bound its pane and is live). */
export interface SessionOpenedEvent {
  kind: 'session_opened';
  session_id: SessionId;
}

/** A session was closed; its pane/PTY is no longer attachable until reopened. */
export interface SessionClosedEvent {
  kind: 'session_closed';
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
 * A previously-requested tool permission was resolved (the correlated
 * `tool_result` was ingested). Auto-approved tools resolve almost immediately,
 * so the notice clears promptly; a genuine TUI prompt yields no result until the
 * human answers, so its notice persists until then.
 */
export interface PermissionResolvedEvent {
  kind: 'permission_resolved';
  session_id: SessionId;
  request_id: number;
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
  | SessionOpenedEvent
  | SessionClosedEvent
  | TurnStartedEvent
  | ExternalInputEvent
  | TurnCompletedEvent
  | PermissionRequestedEvent
  | PermissionResolvedEvent
  | TranscriptUpdatedEvent;

export type SessionEventKind = SessionEvent['kind'];
