import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  MessagesResponse,
  PendingSend,
  SendRequest,
  SendResponse,
  SessionResponse,
  Thread,
  ThreadsResponse,
} from '@delta/model';
import { seedData } from './fixtures';

/**
 * MSW handlers backing the REST surface. They share a small in-memory store so
 * that a `POST /api/sends` that branches actually creates a thread the
 * navigator can then list — making the mock feel like a real session.
 */
export function createHandlers(): RequestHandler[] {
  const store = seedData();

  return [
    http.get('*/api/session', () => {
      const body: SessionResponse = {
        session: store.session,
        main_thread_id: store.threads[0].id,
      };
      return HttpResponse.json(body);
    }),

    http.get('*/api/threads', () => {
      const body: ThreadsResponse = { threads: store.threads };
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

      let threadId = payload.thread_id;
      // A branch send creates a new unnamed child thread off the parent message.
      if (payload.semantic_parent_uuid) {
        const child: Thread = {
          id: store.nextThreadId++,
          session_id: store.session.id,
          title: 'new branch',
          parent_thread_id: payload.thread_id,
          root_message_uuid: payload.semantic_parent_uuid,
          created_at: new Date().toISOString(),
        };
        store.threads.push(child);
        store.messagesByThread[child.id] = [];
        threadId = child.id;
      }

      const send: PendingSend = {
        id: store.nextSendId++,
        session_id: store.session.id,
        thread_id: threadId,
        semantic_parent_uuid: payload.semantic_parent_uuid ?? null,
        text: payload.text,
        locator_quote: payload.locator_quote ?? null,
        status: 'pending',
        matched_uuid: null,
        created_at: new Date().toISOString(),
      };
      store.sends.push(send);
      const body: SendResponse = { send };
      return HttpResponse.json(body, { status: 201 });
    }),
  ];
}

/** Default handler set for tests and the dev server. */
export const handlers: RequestHandler[] = createHandlers();
