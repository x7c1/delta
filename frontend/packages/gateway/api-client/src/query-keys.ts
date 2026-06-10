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
  /**
   * One level of the working-directory browse (`GET /api/workdir/list`). A
   * `null` path means "the server default ($HOME)", keyed distinctly from any
   * concrete path so descending into and back out of it stays cache-stable.
   */
  workdirList: (path: string | null) =>
    ['workdir-list', path ?? 'default'] as const,
  /** Recently-used working directories (`GET /api/workdir/recent`). */
  workdirRecent: ['workdir-recent'] as const,
};
