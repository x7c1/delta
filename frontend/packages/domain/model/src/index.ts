// Pure domain types for Delta. These mirror the wire JSON shapes documented in
// docs/guides/api.md exactly. No React, no fetch, no side effects here.
//
// This is the package entry barrel: it only re-exports the concern modules.
// Each domain concept lives in its own file alongside its closely related
// helpers and id type aliases.

export type { SessionId, SessionStatus, Session } from './session';
export type { ThreadId, Thread } from './thread';
export {
  buildThreadTree,
  threadAncestry,
  type ThreadNode,
} from './thread-tree';
export type {
  TextBlock,
  ThinkingBlock,
  ToolUseBlock,
  ToolResultBlock,
  OtherBlock,
  ContentBlock,
} from './content-block';
export type {
  MessageUuid,
  PromptId,
  MessageRole,
  Message,
} from './message';
export type {
  PendingSendId,
  PendingSendStatus,
  PendingSend,
} from './pending-send';
export type {
  SessionListItem,
  SessionsResponse,
  SessionLifecycle,
  NewSessionResponse,
  ThreadsResponse,
  MessagesResponse,
  SendResponse,
  SendToThread,
  SendToNewSession,
  SendRequest,
  WorkdirEntry,
  WorkdirListResponse,
  RecentWorkdirItem,
  WorkdirRecentResponse,
} from './responses';
export type {
  SessionRegisteredEvent,
  SessionOpenedEvent,
  SessionClosedEvent,
  TurnStartedEvent,
  ExternalInputEvent,
  TurnCompletedEvent,
  PermissionRequestedEvent,
  TranscriptUpdatedEvent,
  SessionEvent,
  SessionEventKind,
} from './session-event';
