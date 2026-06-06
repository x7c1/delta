// Pure domain types for Delta. These mirror the wire JSON shapes documented in
// docs/guides/api.md exactly. No React, no fetch, no side effects here.

// --- ids -------------------------------------------------------------------

/** Server-issued integer identifier for a thread. */
export type ThreadId = number;

/** String identifier for a session. */
export type SessionId = string;

/** String identifier for a transcript message. */
export type MessageUuid = string;

/** String identifier for a prompt. */
export type PromptId = string;

/** Server-issued integer identifier for a pending send. */
export type PendingSendId = number;

// --- session ---------------------------------------------------------------

export type SessionStatus = 'active' | 'ended';

export interface Session {
  id: SessionId;
  cwd: string;
  transcript_path: string;
  title: string | null;
  status: SessionStatus;
  created_at: string;
}

// --- threads ---------------------------------------------------------------

export interface Thread {
  id: ThreadId;
  session_id: SessionId;
  title: string;
  parent_thread_id: ThreadId | null;
  root_message_uuid: MessageUuid | null;
  created_at: string;
}

// --- content blocks --------------------------------------------------------

export interface TextBlock {
  type: 'text';
  text: string;
}

export interface ThinkingBlock {
  type: 'thinking';
  thinking: string;
}

export interface ToolUseBlock {
  type: 'tool_use';
  id: string;
  name: string;
  input: unknown;
}

export interface ToolResultBlock {
  type: 'tool_result';
  tool_use_id: string;
  content: unknown;
  is_error: boolean;
}

/** Any block kind the server does not model is preserved as `{ type: 'other' }`. */
export interface OtherBlock {
  type: 'other';
}

export type ContentBlock =
  | TextBlock
  | ThinkingBlock
  | ToolUseBlock
  | ToolResultBlock
  | OtherBlock;

// --- messages --------------------------------------------------------------

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

// --- pending sends ---------------------------------------------------------

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

// --- REST response envelopes ----------------------------------------------

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

// --- live session events (`/ws`) ------------------------------------------

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

// --- derived view types ----------------------------------------------------

/**
 * A node in the thread navigator tree. Derived from the flat `Thread[]` list;
 * children are ordered by creation (ascending id), matching the server order.
 */
export interface ThreadNode {
  thread: Thread;
  children: ThreadNode[];
}

/**
 * Build a forest of {@link ThreadNode}s from the flat thread list. Roots are
 * threads with no parent (or whose parent is absent). Siblings preserve the
 * input order, which the server guarantees is ascending creation order.
 */
export function buildThreadTree(threads: Thread[]): ThreadNode[] {
  const nodes = new Map<ThreadId, ThreadNode>();
  for (const thread of threads) {
    nodes.set(thread.id, { thread, children: [] });
  }
  const roots: ThreadNode[] = [];
  for (const thread of threads) {
    const node = nodes.get(thread.id)!;
    const parentId = thread.parent_thread_id;
    const parent = parentId === null ? undefined : nodes.get(parentId);
    if (parent) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

/**
 * Walk from a thread up to the root, returning the ancestor chain ordered
 * root-first (so the last element is the thread itself). Used to render the
 * transcript breadcrumb. Threads whose parent is missing terminate the walk.
 */
export function threadAncestry(
  threads: Thread[],
  threadId: ThreadId,
): Thread[] {
  const byId = new Map<ThreadId, Thread>();
  for (const thread of threads) {
    byId.set(thread.id, thread);
  }
  const chain: Thread[] = [];
  let current = byId.get(threadId);
  const seen = new Set<ThreadId>();
  while (current && !seen.has(current.id)) {
    seen.add(current.id);
    chain.push(current);
    current =
      current.parent_thread_id === null
        ? undefined
        : byId.get(current.parent_thread_id);
  }
  return chain.reverse();
}
