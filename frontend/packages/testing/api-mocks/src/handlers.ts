import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  MessagesResponse,
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
  WorkdirListResponse,
  WorkdirRecentResponse,
  Turn,
} from '@delta/wire-gen';
import {
  MOCK_WORKDIR_HOME,
  mockSpawnSessionId,
  recentWorkdirs,
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
   * - `session_registered` activates a `spawning` row (it becomes listable, open);
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

  const handlers: RequestHandler[] = [
    http.get('*/api/sessions', ({ request }) => {
      // A still-`spawning` row stays out of the list, mirroring the real
      // server: it becomes listable when `session_registered` activates it
      // (see `applyEvent`). Mock spawns never hold messages before that, so
      // the server's message-guard has no mock counterpart.
      const items = store.sessions
        .filter((entry) => !entry.spawning)
        .map((entry) => ({
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
      // Derive the turn state the way the server reports it: a `dispatched`
      // send is the one outstanding dispatch awaiting its echo (`in_flight`
      // only begins at the echo match, which the mock cannot observe); with
      // none outstanding, the session is idle.
      const outstanding = sends.find((send) => send.status === 'dispatched');
      const turn: Turn = outstanding
        ? { state: 'awaiting_echo', send_id: outstanding.id }
        : { state: 'idle', send_id: null };
      const body: SendsResponse = { sends, turn };
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

    http.post('*/api/sends', async ({ request }) => {
      const payload = (await request.json()) as SendRequest;
      if (typeof payload?.text !== 'string' || payload.text.length === 0) {
        return HttpResponse.json(
          { error: 'text is required' },
          { status: 422 },
        );
      }

      // New-session target: mirror the server's eager rows. The session row
      // (`spawning`, unlisted until registered), its `main` thread, and the
      // send are all created before the response, so the returned send carries
      // real session/thread/send ids. `locator_quote` is ignored for this
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
        status: 'dispatched',
        matched_uuid: null,
        created_at: new Date().toISOString(),
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

  const applyEvent = (event: SessionEvent): void => {
    switch (event.kind) {
      case 'turn_started': {
        // The named send correlated with its transcript line: terminal.
        const send = store.sends.find((s) => s.id === event.send_id);
        if (send && (send.status === 'queued' || send.status === 'dispatched')) {
          send.status = 'matched';
          send.matched_uuid = event.matched_uuid;
        }
        break;
      }
      case 'turn_completed':
        // The mock has no transcript ingestion, so turn completion is the
        // moment its sends resolve (the real server matches them as the
        // transcript lands during the turn).
        resolveOpenSends(event.session_id, 'matched');
        break;
      case 'turn_interrupted':
        resolveOpenSends(event.session_id, 'cancelled');
        break;
      case 'session_registered': {
        // The spawn bound: the row activates and becomes listable, with a
        // live pane — exactly what the real registration implies.
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
