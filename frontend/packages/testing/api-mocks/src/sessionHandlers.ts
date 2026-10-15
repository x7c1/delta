import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  MessagesResponse,
  SendsResponse,
  SessionsResponse,
  ThreadsResponse,
  Turn,
} from '@delta/wire-gen';
import { SESSIONS_PAGE_SIZE } from './sessionFixtures';
import type { HandlerContext } from './handlerContext';

/**
 * The response the real server gives when a closed session cannot be resumed
 * because its transcript is gone: `409` with the stable `resume_unavailable`
 * code the frontend branches on. Shared by the `open` and `sends` handlers.
 */
export function resumeUnavailableResponse() {
  return HttpResponse.json(
    {
      error: 'session cannot be resumed (transcript missing)',
      code: 'resume_unavailable',
    },
    { status: 409 },
  );
}

/**
 * The `reason` the mock reports when a still-starting session is closed,
 * matching the server's wording (`closed while starting`) so a client's
 * rendering of it can be exercised in mock mode.
 */
export const MOCK_CLOSED_WHILE_STARTING_REASON = 'closed while starting';

/**
 * The session routes: the cursor-paginated list, the lifecycle (spawn, open,
 * close, remove), a session's threads, sends and messages, and the permission
 * and question answers.
 */
export function sessionHandlers({
  store,
  lastActivityAt,
  emitServerEvent,
}: HandlerContext): RequestHandler[] {
  return [
    http.get('*/api/sessions', ({ request }) => {
      // Every row is listed, a still-`spawning` one included, mirroring the
      // real server: a session appears the moment its first send is accepted,
      // reading `status: 'spawning'` (and `open: false` — no pane is bound to
      // it yet) until `session_registered` activates it. A spawn that never
      // binds stays listed too, reading `failed` once `spawn_failed` marks it
      // here — see `applyEvent` for both transitions.
      const items = store.sessions.map((entry) => ({
        session: entry.session,
        open: entry.open,
        // The mock never launches a real pane, so no seeded row is in the
        // launched-but-unbound window; a scenario that wants one sets the flag
        // on its store entry.
        pane_starting: entry.paneStarting ?? false,
        // The mock never restarts, so no session is ever a re-adopted pane
        // whose hooks stopped arriving.
        hooks_unreachable: false,
        main_thread_id: entry.mainThreadId,
        // The same array the threads route below returns: the list carries
        // each session's threads in that route's shape and order.
        threads: entry.threads,
        last_activity_at: lastActivityAt(entry.threads.map((t) => t.id)),
      }));
      // Open-first, mirroring the backend: every live session leads, then the
      // closed ones. "Live" is wider than `open` — a still-`spawning` row counts
      // (the backend reads it off the in-flight spawn), so a just-started
      // session leads the list before anything binds to it.
      const live = (item: (typeof items)[number]) =>
        item.open || item.session.status === 'spawning';
      // Within each group, most-recently-active first: key on last activity,
      // falling back to the session's own created_at when it has no messages,
      // with a deterministic created_at-then-id tiebreaker. ISO-8601 UTC strings
      // compare lexicographically, so a string compare is a time compare.
      const recency = (item: (typeof items)[number]) =>
        item.last_activity_at ?? item.session.created_at;
      items.sort((a, b) => {
        if (live(a) !== live(b)) {
          return live(a) ? -1 : 1;
        }
        if (recency(a) !== recency(b)) {
          return recency(a) < recency(b) ? 1 : -1;
        }
        if (a.session.created_at !== b.session.created_at) {
          return a.session.created_at < b.session.created_at ? 1 : -1;
        }
        return a.session.id < b.session.id ? -1 : a.session.id > b.session.id ? 1 : 0;
      });
      const liveCount = items.filter(live).length;

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

      // `limit` bounds the *closed* portion of a page, as on the real server:
      // the first page additionally carries the whole live group (which sits at
      // the head of `items`), later pages carry `limit` closed rows each.
      const pageSize = offset === 0 ? liveCount + limit : limit;
      const page = items.slice(offset, offset + pageSize);
      const nextOffset = offset + page.length;
      const next_cursor = nextOffset < items.length ? String(nextOffset) : null;

      const body: SessionsResponse = { sessions: page, next_cursor };
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
      // Closing a session that is STILL STARTING cancels its launch instead of
      // tearing a pane down, exactly as on the server: the row was created
      // eagerly when its first send was accepted and holds no conversation, so
      // it is marked `failed` — kept, with its undelivered sends — and the
      // cancellation is reported as a `spawn_failed` carrying the reason (see
      // `MockApi.onServerEvent` for why the mock emits it rather than only
      // mutating the store).
      if (entry.session.status === 'spawning') {
        entry.session.status = 'failed';
        entry.session.failure_reason = MOCK_CLOSED_WHILE_STARTING_REASON;
        entry.spawning = false;
        emitServerEvent({
          kind: 'spawn_failed',
          session_id: entry.session.id,
          reason: MOCK_CLOSED_WHILE_STARTING_REASON,
          // The user asked for this one, which is what tells a client to word
          // it as a cancel rather than a failure.
          cancelled: true,
        });
        return new HttpResponse(null, { status: 204 });
      }
      entry.open = false;
      return new HttpResponse(null, { status: 204 });
    }),

    // Remove a closed session. The two refusals mirror the server's, each with
    // its own stable code, so a stale card meets the same answer in mock mode
    // as in production. The store mutation rides on the emitted
    // `session_removed` (see `MockApi.onServerEvent`), the way the cancelled
    // launch above rides on its `spawn_failed`.
    http.delete('*/api/sessions/:id', ({ params }) => {
      const entry = store.sessions.find((s) => s.session.id === params.id);
      if (!entry) {
        return HttpResponse.json({ error: 'unknown session' }, { status: 404 });
      }
      if (entry.session.status === 'spawning') {
        return HttpResponse.json(
          { error: 'session is still starting', code: 'session_spawning' },
          { status: 409 },
        );
      }
      if (entry.open) {
        return HttpResponse.json(
          { error: 'session is open', code: 'session_open' },
          { status: 409 },
        );
      }
      emitServerEvent({
        kind: 'session_removed',
        session_id: entry.session.id,
      });
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
    // a 404, so a removed session is distinguishable from "nothing pending".
    // A failed launch keeps its row, so it answers here like any other session.
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
  ];
}
