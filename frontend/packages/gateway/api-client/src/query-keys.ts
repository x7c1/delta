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
  /** A single session's open sends (`GET /api/sessions/{id}/sends`). */
  sessionSends: (sessionId: SessionId) => ['session-sends', sessionId] as const,
  /** Placeholder key used while no session is focused (query disabled). */
  sessionSendsNone: ['session-sends', 'none'] as const,
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
  /** Registered repositories (`GET /api/repositories`) for the Repository tab. */
  repositories: ['repositories'] as const,
  /**
   * Open pull requests for the new-session PR tab
   * (`GET /api/prs?lens=…`), one cache entry per lens so flipping
   * between the two sections does not invalidate the other.
   */
  pullRequests: (lens: 'reviewer' | 'author') =>
    ['pull-requests', lens] as const,
  /**
   * Whether a selected directory is a git repository (`GET /api/workdir/git`),
   * keyed by the queried path so each selection caches independently.
   */
  gitRepoInfo: (path: string) => ['git-repo-info', path] as const,
  /**
   * A repository's remote branches (`GET /api/workdir/git/branches`), keyed by
   * the queried path. Fetched lazily (it performs a `git fetch`).
   */
  gitBranches: (path: string) => ['git-branches', path] as const,
  /** The registered launch options (`GET /api/launch-options`). */
  launchOptions: ['launch-options'] as const,
  /** The registered repository scan roots (`GET /api/repository-scan-roots`). */
  repositoryScanRoots: ['repository-scan-roots'] as const,
  /** The Delta workspace version (`GET /api/version`) for the navigator footer. */
  version: ['version'] as const,
  /**
   * Per-provider launch availability (`GET /api/providers`) for the new-session
   * selector. A single cache entry: the answer is host-level, not per-session.
   */
  providers: ['providers'] as const,
};
