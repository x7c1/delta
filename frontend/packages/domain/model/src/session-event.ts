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

/**
 * The in-flight turn was interrupted by the user (Escape / Ctrl-C). Claude's
 * `Stop` hook does not fire on interrupt, so no `turn_completed` arrives and the
 * optimistic "pending send" chip would stay "in progress" forever. The backend
 * detects the `[Request interrupted by user...]` transcript line independently
 * of any hook and emits this so the stuck pending send can be cleared.
 */
export interface TurnInterruptedEvent {
  kind: 'turn_interrupted';
  session_id: SessionId;
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

/**
 * A freshly-spawned session failed to come up: its launch ended (or hung) before
 * it ever registered, so it never bound to a live session. The backend emits
 * this from the `SessionEnd` hook (the launch exited while still unbound) or
 * from the watchdog reaper (the spawn outlived its deadline without binding), so
 * a new session can no longer stall on "pending" forever with no error. Carries
 * the minted `session_id` to correlate with the optimistic pending chip and the
 * `pane_token` of the torn-down tmux session.
 *
 * NOTE: this is currently a passthrough-only event — recognised and parsed so an
 * up-to-date backend does not get its frames dropped, but not yet rendered. The
 * failed-chip UI is a deliberate follow-up.
 */
export interface SpawnFailedEvent {
  kind: 'spawn_failed';
  session_id: SessionId;
  pane_token: string;
}

export type SessionEvent =
  | SessionRegisteredEvent
  | SessionOpenedEvent
  | SessionClosedEvent
  | TurnStartedEvent
  | ExternalInputEvent
  | TurnCompletedEvent
  | TurnInterruptedEvent
  | PermissionRequestedEvent
  | PermissionResolvedEvent
  | TranscriptUpdatedEvent
  | SpawnFailedEvent;

export type SessionEventKind = SessionEvent['kind'];
