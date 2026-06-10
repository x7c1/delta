import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  MessagesResponse,
  NewSessionResponse,
  PendingSend,
  SendRequest,
  SendResponse,
  SendToNewSession,
  SendToThread,
  SessionsResponse,
  Thread,
  ThreadsResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/model';
import {
  MOCK_WORKDIR_HOME,
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
 * MSW handlers backing the multi-session REST surface. They share a small
 * in-memory store (one per {@link createHandlers} call) so a `POST /api/sends`
 * that branches actually creates a thread the navigator can then list, and the
 * open/close endpoints flip a session's live flag — making the mock feel like a
 * real multi-session backend.
 */
export function createHandlers(): RequestHandler[] {
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

  return [
    http.get('*/api/sessions', ({ request }) => {
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

    http.post('*/api/sends', async ({ request }) => {
      const payload = (await request.json()) as SendRequest;
      if (typeof payload?.text !== 'string' || payload.text.length === 0) {
        return HttpResponse.json(
          { error: 'text is required' },
          { status: 422 },
        );
      }

      // New-session target: the spawn has no thread yet, so the server returns a
      // synthetic placeholder send (id 0, empty session id, thread 0) until the
      // session registers. `locator_quote` is ignored for this target.
      if (isNewSessionSend(payload)) {
        const send: PendingSend = {
          id: 0,
          session_id: '',
          thread_id: 0,
          semantic_parent_uuid: null,
          text: payload.text,
          locator_quote: null,
          status: 'pending',
          matched_uuid: null,
          created_at: new Date().toISOString(),
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

      const send: PendingSend = {
        id: store.nextSendId++,
        session_id: session.session.id,
        thread_id: threadId,
        semantic_parent_uuid: target.semantic_parent_uuid ?? null,
        text: target.text,
        locator_quote: target.locator_quote ?? null,
        status: 'pending',
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
}

/** Default handler set for tests and the dev server. */
export const handlers: RequestHandler[] = createHandlers();
