import type { Message, MessageUuid } from './message';
import type { PendingSend } from './pending-send';
import type { Session } from './session';
import type { Thread, ThreadId } from './thread';

/**
 * One entry in `GET /api/sessions`: a session plus its live open/closed flag and
 * the id of its main thread. `open` is in-memory server state — after a restart
 * every persisted session is closed until it is resumed.
 */
export interface SessionListItem {
  session: Session;
  open: boolean;
  main_thread_id: ThreadId;
  /**
   * The timestamp of the session's most recent message (UTC ISO-8601), or
   * `null` when the session has no messages yet.
   */
  last_activity_at: string | null;
}

/**
 * Response body for `GET /api/sessions`, ordered by most recent activity
 * (newest first). The recency key is each session's last activity, falling back
 * to its own `created_at` when it has no messages yet.
 */
export interface SessionsResponse {
  sessions: SessionListItem[];
}

/** Lifecycle state of a Claude Code session after a `new` spawn. */
export type SessionLifecycle = 'ready' | 'starting';

/** Response body for `POST /api/sessions` (eager spawn of a new session). */
export interface NewSessionResponse {
  status: SessionLifecycle;
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

/**
 * Send target addressing an existing session via one of its threads. A branch
 * send additionally sets `semantic_parent_uuid` (and still requires `thread_id`).
 */
export interface SendToThread {
  thread_id: ThreadId;
  text: string;
  locator_quote?: string;
  semantic_parent_uuid?: MessageUuid;
}

/**
 * Send target that spawns a brand-new session. The first message lands on the
 * new session's main thread; there is no `thread_id` yet. `locator_quote` is
 * ignored by the server for this target, so it is intentionally not modelled.
 */
export interface SendToNewSession {
  new_session: true;
  text: string;
}

/** Request body for `POST /api/sends` — a discriminated send target. */
export type SendRequest = SendToThread | SendToNewSession;
