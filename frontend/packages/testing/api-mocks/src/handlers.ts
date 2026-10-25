import type { RequestHandler } from 'msw';
import type {
  PendingPermission,
  PendingQuestion,
  PruneStatus,
  RunningSubagent,
  SessionEvent,
  RepositoriesResponse,
} from '@delta/wire-gen';
import { seedData, type MockStore } from './fixtures';
import type { HandlerContext } from './handlerContext';
import { sessionHandlers } from './sessionHandlers';
import { workdirHandlers } from './workdirHandlers';
import { sendHandlers } from './sendHandlers';
import { repositoryHandlers } from './repositoryHandlers';
import { providerHandlers } from './providerHandlers';
import { launchOptionHandlers } from './launchOptionHandlers';
import { promptTemplateHandlers } from './promptTemplateHandlers';
import { releaseHandlers } from './releaseHandlers';
import { storageHandlers } from './storageHandlers';

/**
 * One mock backend instance: the MSW handlers plus the event mirror that keeps
 * the shared in-memory store consistent with a scripted `/ws` stream.
 */
export interface MockApi {
  /** MSW handlers backing the multi-session REST surface. */
  handlers: RequestHandler[];
  /**
   * Mirror a live `SessionEvent` into the mock REST state, standing in for the
   * server-side transitions the event implies. The real server's transcript
   * ingestion resolves sends and its lifecycle hooks activate spawns; the mock
   * has neither, so the scripted event itself is the moment the store moves:
   *
   * - `turn_started` matches the named send, if it names one (terminal — it
   *   leaves the open list);
   * - `turn_completed` matches every open send of the session;
   * - `turn_interrupted` cancels every open send of the session;
   * - `session_registered` activates a `spawning` row (already listed; it now
   *   reads `active` and becomes open);
   * - `session_opened` / `session_closed` flip the live flag;
   * - `spawn_failed` marks the spawned row `failed` and records its reason,
   *   exactly as the server does for a spawn that never bound (the row and its
   *   undelivered sends are kept);
   *
   * Drive this with the same events fed to the fake event source, *before*
   * queries refetch, so a `GET` that follows an event observes the new state.
   */
  applyEvent: (event: SessionEvent) => void;
  /**
   * Subscribe to the events this mock backend produces *itself* — the mirror
   * image of {@link applyEvent}, which feeds scripted events in.
   *
   * The real server answers some requests by broadcasting on the live channel:
   * closing a still-starting session cancels its launch and reports a
   * `spawn_failed`. The mock has to do the same, or a client would never see
   * the outcome of a request it made. Every emitted event has already been
   * mirrored into the store through {@link applyEvent}, so a listener may pass
   * it straight to the app's event plumbing.
   *
   * Returns an unsubscribe function.
   */
  onServerEvent: (listener: (event: SessionEvent) => void) => () => void;
}

/**
 * Build a mock backend: MSW handlers over a small in-memory store (one per
 * call) so a `POST /api/sends` that branches actually creates a thread the
 * navigator can then list, the open/close endpoints flip a session's live
 * flag, and a new-session send eagerly creates an addressable `spawning` row —
 * making the mock feel like the real multi-session backend.
 */
export function createMockApi(): MockApi {
  const store: MockStore = seedData();

  /** Subscribers to the events this mock backend produces itself. */
  const serverEventListeners = new Set<(event: SessionEvent) => void>();

  const findSessionByThread = (threadId: number) =>
    store.sessions.find((entry) =>
      entry.threads.some((t) => t.id === threadId),
    );

  // The latest message timestamp across a session's threads, or null when it has
  // no messages — mirrors the backend's MAX(message.created_at) derivation.
  const lastActivityAt = (threadIds: number[]): string | null => {
    let latest: string | null = null;
    for (const threadId of threadIds) {
      for (const message of store.messagesByThread[threadId] ?? []) {
        if (latest === null || message.created_at > latest) {
          latest = message.created_at;
        }
      }
    }
    return latest;
  };

  /**
   * Repository-tab entries for the clones made during this mock session, so a
   * landed clone shows up in `GET /api/repositories` the way a real one would.
   */
  const clonedRepositoryEntries = (): RepositoriesResponse['repositories'] =>
    store.clonedRepos.map((entry) => ({
      identity_key: `github.com/${entry.key}`,
      display_name: entry.key,
      recently_used_clone_path: entry.path,
      clones: [
        {
          path: entry.path,
          // A just-cloned repository has never been launched in, which is
          // exactly what a `null` last-opened means.
          last_opened_at: null,
          last_branch: null,
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
      ],
    }));

  /** Whether a session is open or still starting: never removed. */
  const isLive = (entry: MockStore['sessions'][number]) =>
    entry.open || entry.spawning === true || entry.session.status === 'spawning';

  /**
   * The stored sessions the bulk removal's criteria match, oldest first —
   * open ones included, as the server's row query includes them. "Old" is the
   * session list's recency key: last activity, else `created_at`.
   */
  const pruneMatches = (olderThanDays: number, statuses: readonly PruneStatus[]) => {
    const cutoff = new Date(Date.now() - olderThanDays * 86_400_000)
      .toISOString()
      .replace(/\.\d{3}Z$/, 'Z');
    const recency = (entry: MockStore['sessions'][number]) =>
      lastActivityAt(entry.threads.map((t) => t.id)) ?? entry.session.created_at;
    return store.sessions
      .filter((entry) => {
        const status: PruneStatus | null =
          entry.session.status === 'failed'
            ? 'failed'
            : entry.session.status === 'spawning'
              ? null
              : 'ended';
        return status !== null && statuses.includes(status) && recency(entry) <= cutoff;
      })
      .sort((a, b) => recency(a).localeCompare(recency(b)));
  };

  /** Resolve every open (queued/dispatched) send of a session to `status`. */
  const resolveOpenSends = (
    sessionId: string,
    status: 'matched' | 'cancelled',
  ) => {
    for (const send of store.sends) {
      if (
        send.session_id === sessionId &&
        (send.status === 'queued' || send.status === 'dispatched')
      ) {
        send.status = status;
      }
    }
  };

  /** Append a pending dialog to a session's approval queue (de-duplicated). */
  const enqueuePendingPermission = (
    sessionId: string,
    pending: PendingPermission,
  ) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (!entry) {
      return;
    }
    const current = entry.pendingPermissions ?? [];
    if (current.some((p) => p.request_id === pending.request_id)) {
      return;
    }
    entry.pendingPermissions = [...current, pending];
  };

  /**
   * Drop one request from a session's approval queue, leaving the others — the
   * real server's keyed removal, so a resolution for a queued request cannot
   * clear the visible one (and vice versa).
   */
  const resolvePendingPermission = (sessionId: string, requestId: number) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (!entry?.pendingPermissions) {
      return;
    }
    entry.pendingPermissions = entry.pendingPermissions.filter(
      (p) => p.request_id !== requestId,
    );
  };

  /** Drop a session's whole approval queue (a turn end / close sweep). */
  const clearPendingPermissions = (sessionId: string) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (entry) {
      entry.pendingPermissions = undefined;
    }
  };

  const setPendingQuestion = (
    sessionId: string,
    pending: PendingQuestion | undefined,
  ) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (entry) {
      entry.pendingQuestion = pending;
    }
  };

  const setActiveTurn = (
    sessionId: string,
    turn: { sendId: number | null; threadId: number | null } | undefined,
  ) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (entry) {
      entry.activeTurn = turn;
    }
  };

  const startSubagent = (
    sessionId: string,
    subagent: RunningSubagent,
  ) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (!entry) {
      return;
    }
    const current = entry.runningSubagents ?? [];
    if (current.some((s) => s.tool_use_id === subagent.tool_use_id)) {
      return;
    }
    entry.runningSubagents = [...current, subagent];
  };

  const finishSubagent = (sessionId: string, toolUseId: string) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (!entry?.runningSubagents) {
      return;
    }
    entry.runningSubagents = entry.runningSubagents.filter(
      (s) => s.tool_use_id !== toolUseId,
    );
  };

  const clearSubagents = (sessionId: string) => {
    const entry = store.sessions.find((s) => s.session.id === sessionId);
    if (entry) {
      entry.runningSubagents = undefined;
    }
  };

  const applyEvent = (event: SessionEvent): void => {
    switch (event.kind) {
      case 'permission_requested':
        // Mirror the dialog into queryable state, exactly as the real server
        // keeps it in the session's runtime for the sends envelope: appended to
        // the queue, so a second request never overwrites the first.
        enqueuePendingPermission(event.session_id, {
          request_id: event.request_id,
          tool_name: event.tool_name,
          tool_input: event.tool_input,
        });
        break;
      case 'question_asked':
        // Mirror the AskUserQuestion into queryable state, as the real server
        // keeps it for the sends envelope.
        setPendingQuestion(event.session_id, {
          request_id: event.request_id,
          thread_id: event.thread_id,
          tool_input: event.tool_input,
        });
        break;
      case 'permission_resolved':
        // The same event settles a permission dialog and a question whose
        // request id matches (the real server emits it for either row). Keyed
        // for the permission queue: only the answered request leaves, so the
        // next one becomes the envelope's head.
        resolvePendingPermission(event.session_id, event.request_id);
        setPendingQuestion(event.session_id, undefined);
        break;
      case 'subagent_started':
        // Mirror the running subagent into queryable state, as the real server
        // keeps it for the sends envelope (reconnect re-seed).
        startSubagent(event.session_id, {
          thread_id: event.thread_id,
          tool_use_id: event.tool_use_id,
          subagent_type: event.subagent_type,
          description: event.description,
          background: event.background,
        });
        break;
      case 'subagent_finished':
        finishSubagent(event.session_id, event.tool_use_id);
        break;
      case 'turn_started': {
        // The named send, if any, is terminal. A turn no send started (pane
        // typing, a background-task notification) names none.
        const send = store.sends.find((s) => s.id === event.send_id);
        if (send && (send.status === 'queued' || send.status === 'dispatched')) {
          send.status = 'matched';
          send.matched_uuid = event.matched_uuid;
        }
        // The turn is now in flight: keep that queryable until the turn ends,
        // so the sends envelope reports `in_flight` the way the real server
        // does (the matched send alone would read as `idle`).
        setActiveTurn(event.session_id, {
          sendId: event.send_id,
          threadId: event.thread_id,
        });
        break;
      }
      case 'turn_completed':
        // The mock has no transcript ingestion, so turn completion is the
        // moment its sends resolve (the real server matches them as the
        // transcript lands during the turn). A pending dialog cannot outlive
        // its turn, mirroring the server's runtime sweep.
        resolveOpenSends(event.session_id, 'matched');
        setActiveTurn(event.session_id, undefined);
        clearPendingPermissions(event.session_id);
        setPendingQuestion(event.session_id, undefined);
        // A subagent cannot outlive its turn, mirroring the server's sweep.
        clearSubagents(event.session_id);
        break;
      case 'turn_interrupted':
        resolveOpenSends(event.session_id, 'cancelled');
        setActiveTurn(event.session_id, undefined);
        clearPendingPermissions(event.session_id);
        setPendingQuestion(event.session_id, undefined);
        clearSubagents(event.session_id);
        break;
      case 'spawn_pane_ready': {
        // The launch's pane came up while the row is still `spawning`: the real
        // server answers `pane_starting: true` from here, and the app refetches
        // the list on this event to read it. Mirrored so a scenario that scripts
        // the event gets that answer back, rather than the row a browser which
        // never saw it would get.
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry?.spawning) {
          entry.paneStarting = true;
        }
        break;
      }
      case 'session_registered': {
        // The spawn bound: the already-listed row activates and gains a live
        // pane — exactly what the real registration implies. Binding is one end
        // of the starting window, so the attachable-but-unbound flag goes with
        // it: the server reports `pane_starting` and `open` as alternatives,
        // never both.
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry) {
          entry.spawning = false;
          entry.open = true;
          entry.paneStarting = false;
          entry.session.status = 'active';
        }
        break;
      }
      case 'session_opened':
      case 'session_closed': {
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry) {
          entry.open = event.kind === 'session_opened';
          if (event.kind === 'session_closed') {
            // No live process, no dialog — the server clears its runtime
            // mirror when the close drives the turn back to idle.
            entry.pendingPermissions = undefined;
            entry.pendingQuestion = undefined;
            entry.runningSubagents = undefined;
          }
        }
        break;
      }
      case 'spawn_failed': {
        // The launch never bound. The server KEEPS the row, marking it `failed`
        // with the reason on it and leaving its undelivered sends open against
        // it — which is what the failed session's own screen reads back.
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry?.spawning) {
          entry.spawning = false;
          entry.paneStarting = false;
          entry.session.status = 'failed';
          entry.session.failure_reason = event.reason ?? null;
        }
        break;
      }
      case 'session_removed': {
        // The user removed a closed session: the row and every row hanging off
        // it are deleted by the server's cascade. Unconditional, unlike the
        // spawn reap above — this event only ever names a session that is gone.
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry) {
          store.sessions = store.sessions.filter((s) => s !== entry);
          store.sends = store.sends.filter(
            (s) => s.session_id !== event.session_id,
          );
          for (const thread of entry.threads) {
            delete store.messagesByThread[thread.id];
          }
        }
        break;
      }
      case 'repository_clone_completed': {
        // The clone landed. The real server would now find the working tree on
        // disk; the mock records it so the PR and repository lists report it on
        // the refetch the event triggers.
        const key = `${event.repo_owner}/${event.repo_name}`;
        if (!store.clonedRepos.some((entry) => entry.key === key)) {
          store.clonedRepos.push({ key, path: event.destination_path });
        }
        break;
      }
      default:
        break;
    }
  };

  /**
   * Report an event this mock backend produced itself: mirror it into the store
   * first (through the one `applyEvent` switch, so a request-driven event and a
   * scripted one move the store identically), then hand it to the subscribers —
   * in that order, so a listener that triggers a refetch already observes the
   * state the event implies.
   */
  const emitServerEvent = (event: SessionEvent): void => {
    applyEvent(event);
    for (const listener of [...serverEventListeners]) {
      listener(event);
    }
  };

  const onServerEvent = (listener: (event: SessionEvent) => void) => {
    serverEventListeners.add(listener);
    return () => {
      serverEventListeners.delete(listener);
    };
  };

  const context: HandlerContext = {
    store,
    findSessionByThread,
    lastActivityAt,
    clonedRepositoryEntries,
    isLive,
    pruneMatches,
    emitServerEvent,
  };
  const handlers: RequestHandler[] = [
    ...sessionHandlers(context),
    ...workdirHandlers(context),
    ...sendHandlers(context),
    ...repositoryHandlers(context),
    ...providerHandlers(),
    ...launchOptionHandlers(context),
    ...promptTemplateHandlers(context),
    ...releaseHandlers(),
    ...storageHandlers(context),
  ];

  return { handlers, applyEvent, onServerEvent };
}

/**
 * Build only the MSW handlers of a fresh {@link createMockApi} instance, for
 * tests that need no event mirroring.
 */
export function createHandlers(): RequestHandler[] {
  return createMockApi().handlers;
}

/**
 * The shared mock backend for the dev server and the mock-mode app: the MSW
 * worker registers `mockApi.handlers`, and the mock event source mirrors its
 * scripted events through `mockApi.applyEvent` so REST refetches observe the
 * state each event implies.
 */
export const mockApi: MockApi = createMockApi();

/** Default handler set for tests and the dev server. */
export const handlers: RequestHandler[] = mockApi.handlers;
