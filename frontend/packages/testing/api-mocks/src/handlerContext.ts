import type {
  PruneStatus,
  RepositoriesResponse,
  SessionEvent,
} from '@delta/wire-gen';
import type { MockStore } from './fixtures';

/**
 * What one {@link createMockApi} instance hands its per-resource handler sets:
 * its in-memory store and the closures over that store the handlers share.
 */
export interface HandlerContext {
  store: MockStore;
  findSessionByThread: (
    threadId: number,
  ) => MockStore['sessions'][number] | undefined;
  lastActivityAt: (threadIds: number[]) => string | null;
  clonedRepositoryEntries: () => RepositoriesResponse['repositories'];
  isLive: (entry: MockStore['sessions'][number]) => boolean;
  pruneMatches: (
    olderThanDays: number,
    statuses: readonly PruneStatus[],
  ) => MockStore['sessions'];
  emitServerEvent: (event: SessionEvent) => void;
}
