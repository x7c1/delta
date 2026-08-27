import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  CloneRepositoryRequest,
  CreateLaunchOptionRequest,
  CreateCloneRootRequest,
  CreatePromptTemplateRequest,
  LaunchOption,
  LaunchOptionsResponse,
  PromptTemplate,
  PromptTemplatesResponse,
  UpdatePromptTemplateRequest,
  CloneRoot,
  CloneRootsResponse,
  UpdateLaunchOptionRequest,
  MessagesResponse,
  PendingPermission,
  PendingQuestion,
  RunningSubagent,
  NewSessionResponse,
  Send,
  SendRequest,
  SendResponse,
  SendsResponse,
  SessionEvent,
  SessionsResponse,
  SendToNewSession,
  SendToThread,
  Thread,
  ThreadsResponse,
  GitBranchesResponse,
  GitRepoResponse,
  ProvidersResponse,
  PullRequestsResponse,
  RepositoriesResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
  Turn,
} from '@delta/wire-gen';
import {
  gitBranches,
  gitRepoInfo,
  MOCK_VERSION,
  MOCK_WORKDIR_HOME,
  mockSpawnSessionId,
  recentWorkdirs,
  mockAuthorPullRequests,
  mockProviders,
  mockRepositories,
  mockReviewerPullRequests,
  seedData,
  SESSIONS_PAGE_SIZE,
  workdirListing,
  type MockStore,
} from './fixtures';

/** Discriminate a `POST /api/sends` body: new-session spawn vs thread target. */
function isNewSessionSend(body: SendRequest): body is SendToNewSession {
  return 'new_session' in body && body.new_session === true;
}

/**
 * The server's prompt-template validation, mirrored: `label` and `text` are both
 * required and must be non-blank once trimmed. Returns the error message the
 * server would answer with, or `null` when the body is acceptable. Only the
 * check trims — the handlers store what was sent.
 */
function blankPromptTemplateField(body: {
  label?: unknown;
  text?: unknown;
}): string | null {
  if (typeof body?.label !== 'string' || body.label.trim().length === 0) {
    return 'a prompt template must have a non-blank `label`';
  }
  if (typeof body?.text !== 'string' || body.text.trim().length === 0) {
    return 'a prompt template must have non-blank `text`';
  }
  return null;
}

/**
 * Decode a URL-safe base64 token used by the clone-root DELETE path segment,
 * mirroring the server-side decoder. Returns `null` for any non-base64url
 * byte or invalid UTF-8. The implementation uses `atob` after re-padding and
 * substituting the URL-safe variants.
 */
function decodeBase64Url(token: string): string | null {
  // Restore the standard base64 alphabet and re-pad to a multiple of 4 so
  // `atob` can parse it.
  const base64 = token.replace(/-/g, '+').replace(/_/g, '/');
  const padded = base64.padEnd(base64.length + ((4 - (base64.length % 4)) % 4), '=');
  try {
    const binary = atob(padded);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * Where a clone lands: `<clone_root>/<repo_name>`, the server's one rule with
 * no fallback naming.
 */
function cloneDestination(cloneRoot: string, repoName: string): string {
  // Trailing slashes go, so a bare `/` root yields `/<name>` — the same join
  // the server does, and the same one the PR tab predicts.
  return `${cloneRoot.replace(/\/+$/, '')}/${repoName}`;
}

/**
 * Whether the mock filesystem already has a directory at `path`. Standing in
 * for the real server's destination-exists check, using the same little tree the
 * workdir picker browses.
 */
function mockDirectoryExists(path: string): boolean {
  return workdirListing(path) !== null;
}

/**
 * The response the real server gives when a closed session cannot be resumed
 * because its transcript is gone: `409` with the stable `resume_unavailable`
 * code the frontend branches on. Shared by the `open` and `sends` handlers.
 */
function resumeUnavailableResponse() {
  return HttpResponse.json(
    {
      error: 'session cannot be resumed (transcript missing)',
      code: 'resume_unavailable',
    },
    { status: 409 },
  );
}

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
   * - `turn_started` matches the named send (terminal — it leaves the open list);
   * - `turn_completed` matches every open send of the session;
   * - `turn_interrupted` cancels every open send of the session;
   * - `session_registered` activates a `spawning` row (already listed; it now
   *   reads `active` and becomes open);
   * - `session_opened` / `session_closed` flip the live flag;
   * - `spawn_failed` deletes the spawned row and everything it owns, exactly as
   *   the server reaps a spawn that never bound.
   *
   * Drive this with the same events fed to the fake event source, *before*
   * queries refetch, so a `GET` that follows an event observes the new state.
   */
  applyEvent: (event: SessionEvent) => void;
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

  const handlers: RequestHandler[] = [
    http.get('*/api/sessions', ({ request }) => {
      // Every row is listed, a still-`spawning` one included, mirroring the
      // real server: a session appears the moment its first send is accepted,
      // reading `status: 'spawning'` (and `open: false` — no pane is bound to
      // it yet) until `session_registered` activates it. A spawn that never
      // binds is reaped, and `spawn_failed` deletes its row here too — see
      // `applyEvent` for both transitions.
      const items = store.sessions.map((entry) => ({
        session: entry.session,
        open: entry.open,
        main_thread_id: entry.mainThreadId,
        last_activity_at: lastActivityAt(entry.threads.map((t) => t.id)),
      }));
      // Most-recently-active first, mirroring the backend: key on last activity,
      // falling back to the session's own created_at when it has no messages,
      // with a deterministic created_at-then-id tiebreaker. ISO-8601 UTC strings
      // compare lexicographically, so a string compare is a time compare.
      const recency = (item: (typeof items)[number]) =>
        item.last_activity_at ?? item.session.created_at;
      items.sort((a, b) => {
        if (recency(a) !== recency(b)) {
          return recency(a) < recency(b) ? 1 : -1;
        }
        if (a.session.created_at !== b.session.created_at) {
          return a.session.created_at < b.session.created_at ? 1 : -1;
        }
        return a.session.id < b.session.id ? -1 : a.session.id > b.session.id ? 1 : 0;
      });

      // Cursor pagination over the fully-ordered list. The cursor is opaque to
      // the client; here it encodes the offset of the next page's first item.
      // An absent or unparseable cursor starts at offset 0 (first page).
      const url = new URL(request.url);
      const limitParam = url.searchParams.get('limit');
      const parsedLimit = limitParam === null ? NaN : Number(limitParam);
      const requestedLimit =
        Number.isInteger(parsedLimit) && parsedLimit > 0
          ? parsedLimit
          : SESSIONS_PAGE_SIZE;
      // Cap the effective page size to the small mock default so the seeded list
      // always spans multiple pages, even though the app requests a larger
      // production-sized limit. This is what exercises the infinite-scroll path
      // (a non-null next_cursor, then a terminal null) in dev and e2e.
      const limit = Math.min(requestedLimit, SESSIONS_PAGE_SIZE);

      const cursorParam = url.searchParams.get('cursor');
      const parsedOffset = cursorParam === null ? 0 : Number(cursorParam);
      const offset =
        Number.isInteger(parsedOffset) && parsedOffset >= 0 ? parsedOffset : 0;

      const page = items.slice(offset, offset + limit);
      const nextOffset = offset + page.length;
      const next_cursor = nextOffset < items.length ? String(nextOffset) : null;

      const body: SessionsResponse = { sessions: page, next_cursor };
      return HttpResponse.json(body);
    }),

    // Eager spawn. In mock mode the session is considered ready immediately; it
    // does not get added to the list (a real spawn only appears after its first
    // hook binds it via `session_registered`).
    http.post('*/api/sessions', () => {
      const body: NewSessionResponse = { status: 'ready' };
      return HttpResponse.json(body);
    }),

    http.post('*/api/sessions/:id/open', ({ params }) => {
      const entry = store.sessions.find((s) => s.session.id === params.id);
      if (!entry) {
        return HttpResponse.json({ error: 'unknown session' }, { status: 404 });
      }
      // A resume-impossible session stays closed; opening it is refused exactly
      // as the real server's resume gate does.
      if (entry.resumable === false) {
        return resumeUnavailableResponse();
      }
      entry.open = true;
      return new HttpResponse(null, { status: 204 });
    }),

    http.post('*/api/sessions/:id/close', ({ params }) => {
      const entry = store.sessions.find((s) => s.session.id === params.id);
      if (!entry) {
        return HttpResponse.json({ error: 'unknown session' }, { status: 404 });
      }
      entry.open = false;
      return new HttpResponse(null, { status: 204 });
    }),

    http.get('*/api/sessions/:id/threads', ({ params }) => {
      const entry = store.sessions.find((s) => s.session.id === params.id);
      if (!entry) {
        return HttpResponse.json({ error: 'unknown session' }, { status: 404 });
      }
      const body: ThreadsResponse = { threads: entry.threads };
      return HttpResponse.json(body);
    }),

    // A session's open (non-terminal) sends — status queued or dispatched —
    // oldest first, mirroring `GET /api/sessions/{id}/sends`. An unknown id is
    // a 404, so a reaped spawn is distinguishable from "nothing pending".
    http.get('*/api/sessions/:id/sends', ({ params }) => {
      const entry = store.sessions.find((s) => s.session.id === params.id);
      if (!entry) {
        return HttpResponse.json({ error: 'unknown session' }, { status: 404 });
      }
      const sends = store.sends
        .filter(
          (send) =>
            send.session_id === entry.session.id &&
            (send.status === 'queued' || send.status === 'dispatched'),
        )
        .sort((a, b) => a.id - b.id);
      // Derive the turn state the way the server reports it: `turn_started`
      // (mirrored into `activeTurn`) began an in-flight turn that lasts until
      // its completion/interruption; otherwise a `dispatched` send is the one
      // outstanding dispatch awaiting its echo; with neither, the session is
      // idle. Without the `activeTurn` phase the envelope would report `idle`
      // for the whole running turn (its send is already `matched`), and the
      // app's authoritative re-seed (`seedActiveTurn`) would wipe the running
      // flag the `turn_started` event just set — a real-server divergence that
      // surfaced as a flaky running-indicator e2e.
      const outstanding = sends.find((send) => send.status === 'dispatched');
      const turn: Turn = entry.activeTurn
        ? {
            state: 'in_flight',
            send_id: entry.activeTurn.sendId,
            thread_id: entry.activeTurn.threadId,
          }
        : outstanding
          ? {
              state: 'awaiting_echo',
              send_id: outstanding.id,
              thread_id: outstanding.thread_id,
            }
          : { state: 'idle', send_id: null, thread_id: null };
      const body: SendsResponse = {
        sends,
        turn,
        // The pending permission dialog rides along exactly as the real
        // server reports it, so the reconnect re-seed path works in mock mode.
        permission: entry.pendingPermissions?.[0] ?? null,
        // …and its depth, so a reconnecting client can rebuild both the dialog
        // and its "N approvals pending" indication from this one response.
        permission_count: entry.pendingPermissions?.length ?? 0,
        // Likewise the pending AskUserQuestion, so its re-seed path works too.
        question: entry.pendingQuestion ?? null,
        // Likewise the running subagents, so the reconnect re-seed path works.
        running_subagents: entry.runningSubagents ?? [],
      };
      return HttpResponse.json(body);
    }),

    http.get('*/api/threads/:id/messages', ({ params }) => {
      const id = Number(params.id);
      if (!Number.isInteger(id)) {
        return new HttpResponse('invalid thread id', { status: 400 });
      }
      const messages = store.messagesByThread[id];
      if (!messages) {
        return HttpResponse.json({ error: 'unknown thread' }, { status: 404 });
      }
      const body: MessagesResponse = { messages };
      return HttpResponse.json(body);
    }),

    // Answer a pending tool-permission request. The mock has no blocked hook
    // to wake, so it just accepts the decision; the notice clears when the
    // scripted `permission_resolved` event arrives, mirroring the live flow.
    http.post('*/api/permissions/:id/decision', () => {
      return new HttpResponse(null, { status: 204 });
    }),

    // Answer a pending AskUserQuestion. The mock has no real TUI to inject the
    // selection keystrokes into, so it just accepts the answer; the question
    // card clears when the scripted `permission_resolved` event arrives,
    // mirroring the live flow where the `tool_result` resolves the request row.
    http.post('*/api/sessions/:id/questions/:requestId/answer', () => {
      return new HttpResponse(null, { status: 204 });
    }),

    // Cancel a pending AskUserQuestion. The mock has no real TUI to inject the
    // Escape into, so it just accepts the cancel; the question card clears when
    // the scripted `permission_resolved` event arrives, mirroring the live flow
    // where the `is_error` `tool_result` resolves the request row.
    http.post('*/api/sessions/:id/questions/cancel', () => {
      return new HttpResponse(null, { status: 204 });
    }),

    // Open the given path in an external tool (VS Code today). The mock has
    // no real subprocess to spawn, so it just answers `204` on any allowlist
    // hit — a path that appears in a session's cwd or in any stored message's
    // cwd. Anything else answers `400` with the stable code, mirroring the
    // real server's allowlist check.
    http.post('*/api/open-cwd', async ({ request }) => {
      const payload = (await request.json()) as {
        path?: string;
        handler?: string | null;
      };
      const path = typeof payload?.path === 'string' ? payload.path.trim() : '';
      if (path === '') {
        return HttpResponse.json(
          { error: '`path` must be a non-blank string' },
          { status: 400 },
        );
      }
      if (
        typeof payload.handler === 'string' &&
        payload.handler !== 'vscode'
      ) {
        return HttpResponse.json(
          {
            error: `unknown open-cwd handler: ${payload.handler}`,
            code: 'open_cwd_unknown_handler',
          },
          { status: 400 },
        );
      }
      const known = new Set<string>();
      for (const entry of store.sessions) {
        known.add(entry.session.cwd);
      }
      for (const msgs of Object.values(store.messagesByThread)) {
        for (const m of msgs) {
          if (typeof m.cwd === 'string') {
            known.add(m.cwd);
          }
        }
      }
      if (!known.has(path)) {
        return HttpResponse.json(
          {
            error: `path is not in the known-cwd allowlist: ${path}`,
            code: 'open_cwd_path_not_allowed',
          },
          { status: 400 },
        );
      }
      return new HttpResponse(null, { status: 204 });
    }),

    // Cancel a send. The real server cancels `queued` and `dispatched` rows
    // (the row drops out of the open-send list on the next `GET .../sends`)
    // and replies `409` with the stable `send_not_cancellable` code only for
    // an unknown id, an already-terminal row, or a dispatched row whose echo
    // already arrived. The mock has no turn machine to distinguish those
    // `dispatched` sub-cases, so it models only the queued path and 409s
    // everything else.
    http.post('*/api/sends/:id/cancel', ({ params }) => {
      const id = Number(params.id);
      const send = store.sends.find((s) => s.id === id);
      if (!send || send.status !== 'queued') {
        return HttpResponse.json(
          { error: `send ${id} is not cancellable`, code: 'send_not_cancellable' },
          { status: 409 },
        );
      }
      send.status = 'cancelled';
      return new HttpResponse(null, { status: 204 });
    }),

    // Release a held send into the normal queued flow. The real server
    // clears the hold marker only for a still-queued held row (a guarded
    // UPDATE) and 409s everything else with the stable
    // `send_not_releasable` code; the mock mirrors that guard. It performs
    // no dispatch — mock turn progress is driven by scripted events.
    http.post('*/api/sends/:id/release', ({ params }) => {
      const id = Number(params.id);
      const send = store.sends.find((s) => s.id === id);
      if (!send || send.status !== 'queued' || send.held_at === null) {
        return HttpResponse.json(
          {
            error: `send ${id} is not awaiting a release`,
            code: 'send_not_releasable',
          },
          { status: 409 },
        );
      }
      send.held_at = null;
      return new HttpResponse(null, { status: 204 });
    }),

    http.post('*/api/sends', async ({ request }) => {
      const payload = (await request.json()) as SendRequest;
      if (typeof payload?.text !== 'string' || payload.text.length === 0) {
        return HttpResponse.json(
          { error: 'text is required' },
          { status: 422 },
        );
      }

      // New-session target: mirror the server's eager rows. The session row
      // (`spawning`, listed but not open until registered), its `main` thread,
      // and the send are all created before the response, so the returned send
      // carries real session/thread/send ids. `locator_quote` is ignored for this
      // target (a brand-new session has no earlier passage to anchor).
      if (isNewSessionSend(payload)) {
        const sessionId = mockSpawnSessionId(store.nextSpawnOrdinal++);
        const createdAt = new Date().toISOString();
        const mainThread: Thread = {
          id: store.nextThreadId++,
          session_id: sessionId,
          title: 'main',
          parent_thread_id: null,
          root_message_uuid: null,
          created_at: createdAt,
        };
        store.sessions.push({
          session: {
            id: sessionId,
            cwd: payload.workdir ?? MOCK_WORKDIR_HOME,
            // Empty while spawning: the wire keeps the string shape and the
            // real path is only learned from the first hook.
            transcript_path: '',
            title: null,
            status: 'spawning',
            created_at: createdAt,
            // `branch_at_launch`/`repo_root`/`repository_display_name` are
            // captured server-side at the spawn moment. The mock has no real
            // git, so seed all three `null` — mirroring "spawning, no hook
            // yet" / the real server's "non-git launch directory" path. The
            // frontend then falls back to the cwd basename for the repo line
            // and to the session label for the branch line.
            branch_at_launch: null,
            repo_root: null,
            repository_display_name: null,
            // A mock spawn always stands in for a Claude session; Codex
            // provider ids are only minted by the real backend.
            provider: 'claude',
            provider_session_id: null,
            provider_thread_id: null,
          },
          open: false,
          spawning: true,
          mainThreadId: mainThread.id,
          threads: [mainThread],
        });
        store.messagesByThread[mainThread.id] = [];
        const send: Send = {
          id: store.nextSendId++,
          session_id: sessionId,
          thread_id: mainThread.id,
          semantic_parent_uuid: null,
          text: payload.text,
          locator_quote: null,
          status: 'dispatched',
          matched_uuid: null,
          created_at: createdAt,
          held_at: null,
        };
        store.sends.push(send);
        const body: SendResponse = { send };
        return HttpResponse.json(body, { status: 201 });
      }

      // Past the new-session guard the target is a thread send.
      const target: SendToThread = payload;
      const session = findSessionByThread(target.thread_id);
      if (!session) {
        return HttpResponse.json({ error: 'unknown thread' }, { status: 404 });
      }
      // A still-`spawning` session is listed, so its composer is reachable
      // before its launch has bound — and there is nothing to dispatch into
      // yet. The real server still *accepts* a plain send there, recording it
      // as a `queued` row it types once the launch binds (handled below, by the
      // `spawning` branch on the row's status); a **branch** send is the one
      // shape it refuses, because the session has ingested no message to branch
      // from. Mirror exactly that split, or a frontend that mis-handles either
      // half would sail through the suites.
      if (session.spawning && target.semantic_parent_uuid) {
        return HttpResponse.json(
          {
            error: `session is still starting: ${session.session.id}`,
            code: 'session_spawning',
          },
          { status: 409 },
        );
      }
      // Sending to a closed session resumes it first; if that session can no
      // longer be resumed (transcript gone), the send is refused before any
      // optimistic pending row — mirroring the real server.
      if (!session.open && session.resumable === false) {
        return resumeUnavailableResponse();
      }

      let threadId = target.thread_id;
      // A branch send creates a new unnamed child thread off the parent message.
      if (target.semantic_parent_uuid) {
        const child: Thread = {
          id: store.nextThreadId++,
          session_id: session.session.id,
          title: 'new branch',
          parent_thread_id: target.thread_id,
          root_message_uuid: target.semantic_parent_uuid,
          created_at: new Date().toISOString(),
        };
        session.threads.push(child);
        store.messagesByThread[child.id] = [];
        threadId = child.id;
      }

      const send: Send = {
        id: store.nextSendId++,
        session_id: session.session.id,
        thread_id: threadId,
        semantic_parent_uuid: target.semantic_parent_uuid ?? null,
        text: target.text,
        locator_quote: target.locator_quote ?? null,
        // A send accepted while the session is still starting has reached no
        // agent: the server records it `queued` and types it at the bind.
        status: session.spawning ? 'queued' : 'dispatched',
        matched_uuid: null,
        created_at: new Date().toISOString(),
        held_at: null,
      };
      store.sends.push(send);
      const body: SendResponse = { send };
      return HttpResponse.json(body, { status: 201 });
    }),

    // Browse one level of the (mock) filesystem for the new-session picker.
    // An omitted `path` lists $HOME; an unknown path is a 400 and the special
    // `/forbidden` path a 403, exercising the inline-error path.
    http.get('*/api/workdir/list', ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path') ?? MOCK_WORKDIR_HOME;
      if (path === '/forbidden') {
        return HttpResponse.json(
          { error: 'permission denied' },
          { status: 403 },
        );
      }
      const listing = workdirListing(path);
      if (!listing) {
        return HttpResponse.json(
          { error: 'not a directory' },
          { status: 400 },
        );
      }
      const responseBody: WorkdirListResponse = listing;
      return HttpResponse.json(responseBody);
    }),

    http.get('*/api/workdir/recent', () => {
      const responseBody: WorkdirRecentResponse = {
        workdirs: recentWorkdirs(),
      };
      return HttpResponse.json(responseBody);
    }),

    // Registered repositories for the Repository tab. Mirrors the real
    // endpoint's shape; the default list seeds two entries (one
    // origin-deduplicated repo with two clones, one path-keyed single
    // clone) so the picker's clone-expansion + path-key affordance are
    // exercisable.
    http.get('*/api/repositories', () => {
      const responseBody: RepositoriesResponse = {
        repositories: [...mockRepositories(), ...clonedRepositoryEntries()],
      };
      return HttpResponse.json(responseBody);
    }),

    // Clone a repository into a registered clone root. Reproduces the real
    // server's refusals (an unregistered root, an occupied destination) and
    // otherwise answers 202 having done nothing: the mock has no filesystem, so
    // a clone "lands" only when a scripted `repository_clone_completed` event is
    // applied — see `applyEvent`.
    http.post('*/api/repositories/clone', async ({ request }) => {
      const payload = (await request.json()) as CloneRepositoryRequest;
      const cloneRoot = payload?.clone_root ?? '';
      if (!store.cloneRoots.some((root) => root.path === cloneRoot)) {
        return HttpResponse.json(
          {
            error: `not a registered clone root: ${cloneRoot}`,
            code: 'clone_root_not_registered',
          },
          { status: 400 },
        );
      }
      const destination = cloneDestination(cloneRoot, payload.repo_name);
      // The mock's stand-in for the filesystem: the workdir tree plus whatever
      // this session has already cloned. Either counts as "already there".
      const occupied =
        mockDirectoryExists(destination) ||
        store.clonedRepos.some((entry) => entry.path === destination);
      if (occupied) {
        return HttpResponse.json(
          {
            error: `clone destination already exists: ${destination}`,
            code: 'clone_dest_exists',
          },
          { status: 409 },
        );
      }
      return new HttpResponse(null, { status: 202 });
    }),

    // Pull requests for the PR tab. Each lens carries its own canned
    // list (the reviewer fixture pairs a clone-having row with a
    // no-clone row so the inline clone panel is exercisable; the author
    // fixture seeds one of the user's own drafts). An unknown lens is a
    // 400, mirroring the server.
    http.get('*/api/prs', ({ request }) => {
      const url = new URL(request.url);
      const lens = url.searchParams.get('lens');
      const pull_requests =
        lens === 'reviewer'
          ? mockReviewerPullRequests()
          : lens === 'author'
            ? mockAuthorPullRequests()
            : null;
      if (pull_requests === null) {
        return HttpResponse.json(
          { error: `unknown lens '${lens ?? ''}'` },
          { status: 400 },
        );
      }
      const responseBody: PullRequestsResponse = {
        gh_available: true,
        // A repository cloned during this session has a local clone from here
        // on, exactly as the real endpoint would report after the clone landed.
        pull_requests: pull_requests.map((pr) => ({
          ...pr,
          has_local_clone:
            pr.has_local_clone ||
            store.clonedRepos.some(
              (entry) => entry.key === `${pr.repo_owner}/${pr.repo_name}`,
            ),
        })),
      };
      return HttpResponse.json(responseBody);
    }),

    // Per-provider launch availability for the new-session selector. Both
    // providers are available by default so the selector is fully usable with no
    // backend; a test overrides this handler to make a provider unavailable.
    http.get('*/api/providers', () => {
      const responseBody: ProvidersResponse = { providers: mockProviders() };
      return HttpResponse.json(responseBody);
    }),

    // Whether the queried directory is a git repository, for the new-session
    // worktree option. A non-git path is not an error — it reports
    // `repo_root: null` — but `path` is required: a missing or blank one is the
    // same 400 the real endpoint's `require_path` returns, and the value is
    // trimmed before it is resolved. A path under the mock repo reports its root
    // and default branch, so the worktree toggle's show/hide is exercisable.
    http.get('*/api/workdir/git', ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path')?.trim() ?? '';
      if (path.length === 0) {
        return HttpResponse.json(
          { error: 'a `path` query parameter is required' },
          { status: 400 },
        );
      }
      const responseBody: GitRepoResponse = gitRepoInfo(path);
      return HttpResponse.json(responseBody);
    }),

    // The repository's remote branches (the lazily-fetched start-point list). A
    // non-git path is a 400, exactly as the real endpoint rejects it, so the
    // picker's inline-error path is exercisable; a missing or blank `path` is
    // the same 400 for the same reason as above.
    http.get('*/api/workdir/git/branches', ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path')?.trim() ?? '';
      if (path.length === 0) {
        return HttpResponse.json(
          { error: 'a `path` query parameter is required' },
          { status: 400 },
        );
      }
      const branches = gitBranches(path);
      if (!branches) {
        return HttpResponse.json(
          { error: 'not a git repository' },
          { status: 400 },
        );
      }
      const responseBody: GitBranchesResponse = branches;
      return HttpResponse.json(responseBody);
    }),

    // The launch-option registry for the settings screen: list, create, delete.
    // Backed by the shared in-memory store so a created option lists and a
    // deleted one disappears, mirroring the real server's SQLite-backed CRUD.
    // The rows Delta ships are seeded like any other, so the mock exercises the
    // same badge / no-delete / 409 paths the real server produces.
    http.get('*/api/launch-options', () => {
      // The server's order: the rows Delta ships first (ascending id, i.e.
      // declared-catalog order), then the user's own newest first (descending
      // id).
      const launch_options = [...store.launchOptions].sort((a, b) =>
        a.builtin !== b.builtin
          ? Number(b.builtin) - Number(a.builtin)
          : a.builtin
            ? a.id - b.id
            : b.id - a.id,
      );
      const body: LaunchOptionsResponse = { launch_options };
      return HttpResponse.json(body);
    }),

    http.post('*/api/launch-options', async ({ request }) => {
      const payload = (await request.json()) as CreateLaunchOptionRequest;
      const name = typeof payload?.name === 'string' ? payload.name.trim() : '';
      if (name.length === 0) {
        // A blank name is a 400, exactly as the real server rejects it.
        return HttpResponse.json(
          { error: 'a launch option must have a non-blank `name`' },
          { status: 400 },
        );
      }
      const trimmedLabel = payload.label?.trim();
      const trimmedValue = payload.value?.trim();
      const option: LaunchOption = {
        id: store.nextLaunchOptionId++,
        label: trimmedLabel ? trimmedLabel : null,
        name,
        value: trimmedValue ? trimmedValue : null,
        default_enabled: payload.default_enabled === true,
        created_at: new Date().toISOString(),
        // Omitted `provider` defaults to Claude, matching the real server's
        // back-compat behavior.
        provider: payload.provider ?? 'claude',
        // Anything registered through the API is the user's own; only Delta's
        // startup reconcile writes a shipped row.
        builtin: false,
      };
      store.launchOptions.push(option);
      return HttpResponse.json(option, { status: 201 });
    }),

    http.patch('*/api/launch-options/:id', async ({ params, request }) => {
      const id = Number(params.id);
      const payload = (await request.json()) as UpdateLaunchOptionRequest;
      const option = store.launchOptions.find((o) => o.id === id);
      if (!option) {
        // An unknown id is a 404, exactly as the real server reports it.
        return HttpResponse.json(
          { error: `no launch option with id ${id}` },
          { status: 404 },
        );
      }
      option.default_enabled = payload.default_enabled === true;
      return HttpResponse.json(option);
    }),

    http.delete('*/api/launch-options/:id', ({ params }) => {
      const id = Number(params.id);
      const target = store.launchOptions.find((o) => o.id === id);
      if (target?.builtin) {
        // A row Delta ships is not the user's to remove: a 409, and the row
        // stays. The Settings UI omits the control entirely, so this only
        // answers a stale list — but it has to answer it the way the real
        // server does, or that path could never be driven without a backend.
        return HttpResponse.json(
          {
            error: `launch option ${id} is built in and cannot be deleted`,
            code: 'launch_option_builtin',
          },
          { status: 409 },
        );
      }
      // Deleting an unknown id is a no-op (idempotent), like the real server.
      store.launchOptions = store.launchOptions.filter((o) => o.id !== id);
      return new HttpResponse(null, { status: 204 });
    }),

    // The prompt-template registry: list, create, update, delete. Backed by the
    // shared in-memory store so a created template lists, an edit is reflected
    // in place, and a deleted one disappears — the full CRUD a component test
    // or an e2e-fake spec needs without a backend. The registry is global: no
    // provider filtering, unlike launch options.
    http.get('*/api/prompt-templates', () => {
      // Oldest first (ascending created_at, then id), as the server returns them.
      const prompt_templates = [...store.promptTemplates].sort(
        (a, b) => a.created_at.localeCompare(b.created_at) || a.id - b.id,
      );
      const body: PromptTemplatesResponse = { prompt_templates };
      return HttpResponse.json(body);
    }),

    http.post('*/api/prompt-templates', async ({ request }) => {
      const payload = (await request.json()) as CreatePromptTemplateRequest;
      const invalid = blankPromptTemplateField(payload);
      if (invalid) {
        return HttpResponse.json({ error: invalid }, { status: 400 });
      }
      const now = new Date().toISOString();
      const template: PromptTemplate = {
        id: store.nextPromptTemplateId++,
        // Stored verbatim — only the blank check trims, exactly as the server
        // does, so a template that ends with a newline keeps it.
        label: payload.label,
        text: payload.text,
        created_at: now,
        updated_at: now,
      };
      store.promptTemplates.push(template);
      return HttpResponse.json(template, { status: 201 });
    }),

    http.patch('*/api/prompt-templates/:id', async ({ params, request }) => {
      const id = Number(params.id);
      const payload = (await request.json()) as UpdatePromptTemplateRequest;
      // Validation runs before the lookup, matching the server: the use case
      // rejects a blank field before the store is ever asked about the id, so a
      // blank edit of an id that no longer exists is a 400, not a 404.
      const invalid = blankPromptTemplateField(payload);
      if (invalid) {
        return HttpResponse.json({ error: invalid }, { status: 400 });
      }
      const template = store.promptTemplates.find((t) => t.id === id);
      if (!template) {
        // An unknown id is a 404, exactly as the real server reports it.
        return HttpResponse.json(
          { error: `no prompt template with id ${id}` },
          { status: 404 },
        );
      }
      template.label = payload.label;
      template.text = payload.text;
      // Re-stamped on every edit, leaving created_at (and the list position) alone.
      template.updated_at = new Date().toISOString();
      return HttpResponse.json(template);
    }),

    http.delete('*/api/prompt-templates/:id', ({ params }) => {
      const id = Number(params.id);
      // Deleting an unknown id is a no-op (idempotent), like the real server.
      store.promptTemplates = store.promptTemplates.filter((t) => t.id !== id);
      return new HttpResponse(null, { status: 204 });
    }),

    // Clone roots: list, create, delete. The mock reproduces the real server's
    // contract — newest-first ordering, trailing-slash trim, duplicate-path 409
    // with the stable `clone_root_duplicate` code, and an idempotent delete on
    // an unknown path.
    http.get('*/api/clone-roots', () => {
      const clone_roots = [...store.cloneRoots].sort((a, b) =>
        b.created_at.localeCompare(a.created_at) || a.path.localeCompare(b.path),
      );
      const body: CloneRootsResponse = {
        clone_roots: clone_roots.map((root) => ({ path: root.path })),
      };
      return HttpResponse.json(body);
    }),

    http.post('*/api/clone-roots', async ({ request }) => {
      const payload = (await request.json()) as CreateCloneRootRequest;
      const rawPath = typeof payload?.path === 'string' ? payload.path.trim() : '';
      // Only the bare `/` is exempt from the stripping: it is the one all-slash
      // spelling the contract takes as a deliberate root, while `'//'` and
      // `'///'` strip down to nothing, blank like `''` and `'   '`. Same rule
      // as the server's `create_clone_root`.
      const canonical = rawPath === '/' ? '/' : rawPath.replace(/\/+$/, '');
      if (canonical.length === 0) {
        return HttpResponse.json(
          { error: 'a clone root must have a non-blank `path`' },
          { status: 400 },
        );
      }
      if (!canonical.startsWith('/')) {
        return HttpResponse.json(
          { error: 'a clone root `path` must be absolute (start with `/`)' },
          { status: 400 },
        );
      }
      if (store.cloneRoots.some((r) => r.path === canonical)) {
        return HttpResponse.json(
          { error: `clone root already registered: ${canonical}`, code: 'clone_root_duplicate' },
          { status: 409 },
        );
      }
      store.cloneRoots.push({
        path: canonical,
        // Stored only for the newest-first list ordering; stripped from the wire.
        created_at: new Date().toISOString(),
      });
      const row: CloneRoot = { path: canonical };
      return HttpResponse.json(row, { status: 201 });
    }),

    http.delete('*/api/clone-roots/:path_b64', ({ params }) => {
      const token = String(params.path_b64 ?? '');
      const decoded = decodeBase64Url(token);
      if (decoded === null) {
        return HttpResponse.json(
          { error: 'malformed clone-root path token' },
          { status: 400 },
        );
      }
      // Idempotent: an unknown path is still a 204.
      store.cloneRoots = store.cloneRoots.filter((r) => r.path !== decoded);
      return new HttpResponse(null, { status: 204 });
    }),

    // Delta workspace version for the navigator footer. The real server owns
    // the format contract (`v<version>` on release, `v<version>+dev.<sha>` on
    // debug); the mock returns a fixed dev-shaped string so mock-mode e2e can
    // assert on it without depending on the host's git sha.
    http.get('*/api/version', () => HttpResponse.json({ version: MOCK_VERSION })),
  ];

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
    turn: { sendId: number; threadId: number | null } | undefined,
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
        // The named send correlated with its transcript line: terminal.
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
      case 'session_registered': {
        // The spawn bound: the already-listed row activates and gains a live
        // pane — exactly what the real registration implies.
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry) {
          entry.spawning = false;
          entry.open = true;
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
        // The server reaps a spawn that never bound: the contentless session
        // row and everything it owns are deleted.
        const entry = store.sessions.find(
          (s) => s.session.id === event.session_id,
        );
        if (entry?.spawning) {
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

  return { handlers, applyEvent };
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
