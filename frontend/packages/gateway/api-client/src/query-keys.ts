import type { SessionId, ThreadId } from '@delta/model';

/**
 * TanStack Query keys for the REST surface. Centralised here so both the query
 * hooks and the WebSocket-driven cache patchers address the same cache entries.
 */
export const queryKeys = {
  /** The full session list (`GET /api/sessions`). */
  sessions: ['sessions'] as const,
  /** A single session's thread tree (`GET /api/sessions/{id}/threads`). */
  sessionThreads: (sessionId: SessionId) =>
    ['session-threads', sessionId] as const,
  /** Placeholder key used while no session is focused (query disabled). */
  sessionThreadsNone: ['session-threads', 'none'] as const,
  messages: (threadId: ThreadId) => ['messages', threadId] as const,
  /** Placeholder key used while no thread is selected (query disabled). */
  messagesNone: ['messages', 'none'] as const,
};
