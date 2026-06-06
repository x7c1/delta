import type { ThreadId } from '@delta/model';

/**
 * TanStack Query keys for the REST surface. Centralised here so both the query
 * hooks and the WebSocket-driven cache patchers address the same cache entries.
 */
export const queryKeys = {
  session: ['session'] as const,
  threads: ['threads'] as const,
  messages: (threadId: ThreadId) => ['messages', threadId] as const,
};
