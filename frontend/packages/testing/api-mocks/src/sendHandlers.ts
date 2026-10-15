import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  LaunchTrustResponse,
  Send,
  SendRequest,
  SendResponse,
  SendToNewSession,
  SendToThread,
  Thread,
} from '@delta/wire-gen';
import { mockSpawnSessionId } from './fixtures';
import type { HandlerContext } from './handlerContext';
import { resumeUnavailableResponse } from './sessionHandlers';

/** The commit the mock's launch-trust check reports for every launch. */
const MOCK_LAUNCH_COMMIT = 'mock0000launch';

/** Discriminate a `POST /api/sends` body: new-session spawn vs thread target. */
function isNewSessionSend(body: SendRequest): body is SendToNewSession {
  return 'new_session' in body && body.new_session === true;
}

/** The send routes: create, cancel and release. */
export function sendHandlers({
  store,
  findSessionByThread,
}: HandlerContext): RequestHandler[] {
  return [
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

    // The launch-trust check a new worktree session runs before its send. The
    // mock models a user whose repository is trusted and whose branches leave
    // agent configuration alone — the common case, which asks nothing — so the
    // new-session flow goes straight through. A test that needs the
    // confirmation overrides this route with its own answer.
    http.post('*/api/launch-trust', () =>
      HttpResponse.json<LaunchTrustResponse>({
        needs_confirmation: false,
        repository_untrusted: false,
        commit: MOCK_LAUNCH_COMMIT,
        agent_config_changes: [],
      }),
    ),

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
        // Mirror the server's refusal: a session always starts in a directory
        // the user chose, so a new-session send without one is a `400`.
        if (!payload.workdir?.trim()) {
          return HttpResponse.json(
            {
              error:
                'a working directory (`workdir`) is required to start a new session',
            },
            { status: 400 },
          );
        }
        const sessionId = mockSpawnSessionId(store.nextSpawnOrdinal++);
        const createdAt = new Date().toISOString();
        const mainThread: Thread = {
          id: store.nextThreadId++,
          session_id: sessionId,
          title: 'main',
          parent_thread_id: null,
          root_message_uuid: null,
          last_activity_at: null,
          created_at: createdAt,
        };
        store.sessions.push({
          session: {
            id: sessionId,
            cwd: payload.workdir,
            // Empty while spawning: the wire keeps the string shape and the
            // real path is only learned from the first hook.
            transcript_path: '',
            title: null,
            status: 'spawning',
            created_at: createdAt,
            failure_reason: null,
            closed_reason: null,
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
            // The real server snapshots the PR the send named onto the eager
            // `spawning` row, so the card shows it before the launch binds.
            // Mirror that here, or the mocked list would drop the origin the
            // send just carried.
            pull_request_number: payload.pull_request_number ?? null,
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
          last_activity_at: null,
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
  ];
}
