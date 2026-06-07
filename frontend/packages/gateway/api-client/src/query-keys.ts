import type { ThreadId } from '@delta/model';

/**
 * TanStack Query keys for the REST surface. Centralised here so both the query
 * hooks and the WebSocket-driven cache patchers address the same cache entries.
 */
export const queryKeys = {
  /** Lazy ensure-session call made once on app load. */
  ensureSession: ['ensure-session'] as const,
  session: ['session'] as const,
  threads: ['threads'] as const,
  messages: (threadId: ThreadId) => ['messages', threadId] as const,
  /** Placeholder key used while no thread is selected (query disabled). */
  messagesNone: ['messages', 'none'] as const,
};
