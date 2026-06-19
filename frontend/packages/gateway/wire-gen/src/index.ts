// TypeScript bindings generated from the backend's wire contract (the
// `delta-wire` crate). Everything under `generated/` is written by
// `make gen`; the barrel and the small hand-maintained helpers next to it
// (`content-block.ts`, `send-request.ts`) only narrow the generated types,
// never restate the wire shapes.

export type { SessionEvent } from './generated/SessionEvent';
export { EVENT_KINDS, type SessionEventKind } from './generated/event-kinds';
export type { StatusSnapshot } from './generated/StatusSnapshot';
export type { RateLimitWindow } from './generated/RateLimitWindow';

export type { Session } from './generated/Session';
export type { SessionStatus } from './generated/SessionStatus';
export type { Thread } from './generated/Thread';
export type { Message } from './generated/Message';
export type { MessageRole } from './generated/MessageRole';
export type { ContentBlock } from './generated/ContentBlock';
export type { Send } from './generated/Send';
export type { SendStatus } from './generated/SendStatus';

export type { SessionListItem } from './generated/SessionListItem';
export type { SessionsResponse } from './generated/SessionsResponse';
export type { SessionLifecycle } from './generated/SessionLifecycle';
export type { NewSessionResponse } from './generated/NewSessionResponse';
export type { ThreadsResponse } from './generated/ThreadsResponse';
export type { MessagesResponse } from './generated/MessagesResponse';
export type { CreateSendRequest } from './generated/CreateSendRequest';
export type { SendResponse } from './generated/SendResponse';
export type { SendsResponse } from './generated/SendsResponse';
export type { PermissionDecision } from './generated/PermissionDecision';
export type { PermissionDecisionRequest } from './generated/PermissionDecisionRequest';
export type { QuestionAnswerRequest } from './generated/QuestionAnswerRequest';
export type { QuestionCancelRequest } from './generated/QuestionCancelRequest';
export type { Turn } from './generated/Turn';
export type { PendingPermission } from './generated/PendingPermission';
export type { PendingQuestion } from './generated/PendingQuestion';
export type { RunningSubagent } from './generated/RunningSubagent';
export type { TurnPhase } from './generated/TurnPhase';
export type { WorkdirEntry } from './generated/WorkdirEntry';
export type { WorkdirListResponse } from './generated/WorkdirListResponse';
export type { RecentWorkdirItem } from './generated/RecentWorkdirItem';
export type { WorkdirRecentResponse } from './generated/WorkdirRecentResponse';
export type { GitRepoResponse } from './generated/GitRepoResponse';
export type { GitBranchesResponse } from './generated/GitBranchesResponse';
export type { WorktreeSpec } from './generated/WorktreeSpec';
export type { WorktreeStartPoint } from './generated/WorktreeStartPoint';
export type { LaunchOption } from './generated/LaunchOption';
export type { LaunchOptionsResponse } from './generated/LaunchOptionsResponse';
export type { CreateLaunchOptionRequest } from './generated/CreateLaunchOptionRequest';
export type { UpdateLaunchOptionRequest } from './generated/UpdateLaunchOptionRequest';
export type { ErrorBody } from './generated/ErrorBody';

export type {
  TextBlock,
  ThinkingBlock,
  ToolUseBlock,
  ToolResultBlock,
  OtherBlock,
} from './content-block';
export type {
  SendToThread,
  SendToNewSession,
  SendRequest,
} from './send-request';
