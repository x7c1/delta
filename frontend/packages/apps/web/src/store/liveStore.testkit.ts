/**
 * Shared helpers for the `liveStore.*.test.ts` family: the store reset and the
 * builders for status snapshots, rate-limit windows, sends and notices.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import type { StatusSnapshot } from '@delta/wire-gen';
import { useLiveStore, type LocalSend } from './liveStore';
import { useNotificationStore } from './notificationStore';

export function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    sending: [],
    localSends: {},
    spawns: [],
    runningThreads: {},
    notices: {},
    resolvedRequests: {},
    unread: {},
    threadActivity: {},
    streamingMessages: {},
    runningSubagents: {},
    contextUsage: {},
    rateLimits: {},
    statusObservedAt: { contextUsage: {}, rateLimits: {} },
    restoredRateLimitsObservedAt: {},
  });
  // A spawn failure this client never tracked announces itself in the app-wide
  // snackbar, which is shared state these tests move too.
  useNotificationStore.setState({ notifications: [] });
}

/**
 * A usage snapshot, with sensible defaults overridable per field. `rate_limits`
 * defaults to `null` — "this snapshot says nothing about rate limits" — which
 * is what a provider's token-usage-only frame looks like.
 */
export function statusSnapshot(
  overrides: Partial<StatusSnapshot> = {},
): StatusSnapshot {
  return {
    provider: 'claude',
    model_id: null,
    model_display_name: null,
    context_used_percentage: null,
    context_window_size: null,
    context_current_usage: null,
    total_input_tokens: null,
    rate_limits: null,
    total_cost_usd: null,
    current_dir: null,
    ...overrides,
  };
}

/** A rate-limit window of `durationSeconds`, as the wire delivers it. */
export function window(
  durationSeconds: number | null,
  usedPercentage: number | null,
  resetsAt: number | null,
) {
  return {
    duration_seconds: durationSeconds,
    used_percentage: usedPercentage,
    resets_at: resetsAt,
  };
}

export const FIVE_HOURS = 5 * 60 * 60;
export const SEVEN_DAYS = 7 * 24 * 60 * 60;

/** A thread-targeted submit chip, as `beginSending` records before its POST. */
export function beginThreadSending(overrides: { sessionId?: string; threadId?: number } = {}) {
  useLiveStore.getState().beginSending({
    id: `local-${overrides.sessionId ?? 'sess-1'}-${overrides.threadId ?? 1}`,
    target: {
      kind: 'thread',
      sessionId: overrides.sessionId ?? 'sess-1',
      threadId: overrides.threadId ?? 1,
    },
    text: 'hi',
    status: 'sending',
    createdAt: 0,
  });
}

/**
 * A new-session submit chip, as `beginSending` records before its POST. The
 * target carries no session id — the response mints it — so this stands in for
 * the first-turn launch the new-session composer fires.
 */
export function beginNewSessionSending(overrides: { id?: string } = {}) {
  useLiveStore.getState().beginSending({
    id: overrides.id ?? 'local-new-1',
    target: {
      kind: 'new-session',
      workdir: '/projects/app',
      launchOptionIds: [],
      provider: 'claude',
      worktree: null,
      pullRequestNumber: null,
    },
    text: 'hi',
    status: 'sending',
    createdAt: 0,
  });
}

/** The notices map, for `noticeOf` lookups in assertions. */
export function notices() {
  return useLiveStore.getState().notices;
}

export function localSend(overrides: Partial<LocalSend> = {}): LocalSend {
  return {
    sendId: 1,
    sessionId: 'sess-1',
    threadId: 1,
    text: 'hi',
    createdAt: 0,
    ...overrides,
  };
}
