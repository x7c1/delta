import type { SessionId, ThreadId } from '@delta/model';
import type {
  CreateSendRequest,
  MessagesResponse,
  NewSessionResponse,
  PermissionDecision,
  PermissionDecisionRequest,
  QuestionAnswerRequest,
  SendRequest,
  SendResponse,
  SendsResponse,
  SessionsResponse,
  ThreadsResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/wire-gen';

/**
 * The single place in the codebase where `fetch` is allowed. All REST calls to
 * the Delta server are confined to this module and return the generated
 * `@delta/wire-gen` types.
 */

export interface ApiClientOptions {
  /** Base URL for the REST surface. Defaults to a same-origin relative base. */
  baseUrl?: string;
  /** Injectable fetch, primarily for tests. Defaults to the global `fetch`. */
  fetchFn?: typeof fetch;
}

/**
 * Stable machine-readable error code the server may attach to an error body.
 *
 * `resume_unavailable` means a closed session cannot be resumed because its
 * local transcript file is gone, so `claude --resume` has nothing to replay.
 * Callers branch on this to keep the session closed and show a specific message
 * rather than a generic failure.
 *
 * `permission_not_pending` means a permission decision can no longer take
 * effect: the request was already decided, or its hook wait timed out and the
 * interactive TUI prompt owns it now. Callers branch on this to swap the
 * Allow/Deny buttons for the answer-in-the-terminal guidance.
 *
 * `question_not_pending` means an `AskUserQuestion` can no longer be answered
 * from the UI (already answered, its turn ended, or no live pane). Callers
 * branch on this to keep the answer-in-the-terminal fallback.
 */
export type ApiErrorCode =
  | 'resume_unavailable'
  | 'permission_not_pending'
  | 'question_not_pending';

/** An error raised when the server responds with a non-2xx status. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    /** The server's machine-readable error code, when present. */
    readonly code?: ApiErrorCode,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/** The parsed error payload: a human message and an optional stable code. */
interface ParsedError {
  message: string;
  code?: ApiErrorCode;
}

async function readError(response: Response): Promise<ParsedError> {
  const text = await response.text();
  if (!text) {
    return { message: response.statusText };
  }
  try {
    const body = JSON.parse(text) as { error?: string; code?: string };
    return {
      message: body.error ?? text,
      code: body.code as ApiErrorCode | undefined,
    };
  } catch {
    return { message: text };
  }
}

export class ApiClient {
  private readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;

  constructor(options: ApiClientOptions = {}) {
    // Normalise away any trailing slash so we can concatenate paths directly.
    this.baseUrl = (options.baseUrl ?? '').replace(/\/$/, '');
    this.fetchFn = options.fetchFn ?? globalThis.fetch.bind(globalThis);
  }

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await this.fetchFn(`${this.baseUrl}${path}`, init);
    if (!response.ok) {
      const { message, code } = await readError(response);
      throw new ApiError(response.status, message, code);
    }
    return (await response.json()) as T;
  }

  /** Like {@link request} but for endpoints that reply `204 No Content`. */
  private async requestNoContent(
    path: string,
    init?: RequestInit,
  ): Promise<void> {
    const response = await this.fetchFn(`${this.baseUrl}${path}`, init);
    if (!response.ok) {
      const { message, code } = await readError(response);
      throw new ApiError(response.status, message, code);
    }
  }

  /**
   * `GET /api/sessions` — one page of sessions with their open flag and main
   * thread, ordered most-recently-active first. Pass the previous page's
   * `next_cursor` as `cursor` to fetch the following page; omit it for the
   * first page. `limit` caps the page size. The cursor is opaque: echo it back
   * verbatim, never construct or parse one.
   */
  getSessions(
    params?: { cursor?: string; limit?: number },
  ): Promise<SessionsResponse> {
    // `request()` has no query-string helper, so build it manually with the
    // same encodeURIComponent style used by the path-segment endpoints above.
    const query: string[] = [];
    if (params?.cursor !== undefined) {
      query.push(`cursor=${encodeURIComponent(params.cursor)}`);
    }
    if (params?.limit !== undefined) {
      query.push(`limit=${encodeURIComponent(params.limit)}`);
    }
    const suffix = query.length > 0 ? `?${query.join('&')}` : '';
    return this.request<SessionsResponse>(`/api/sessions${suffix}`);
  }

  /**
   * `POST /api/sessions` — eagerly spawn a brand-new session.
   *
   * The session does not appear in `GET /api/sessions` until its first hook
   * binds it (announced via `session_registered`).
   */
  newSession(): Promise<NewSessionResponse> {
    return this.request<NewSessionResponse>('/api/sessions', {
      method: 'POST',
    });
  }

  /** `POST /api/sessions/{id}/open` — resume a closed session (204). */
  openSession(sessionId: SessionId): Promise<void> {
    return this.requestNoContent(
      `/api/sessions/${encodeURIComponent(sessionId)}/open`,
      { method: 'POST' },
    );
  }

  /** `POST /api/sessions/{id}/close` — close an open session (204). */
  closeSession(sessionId: SessionId): Promise<void> {
    return this.requestNoContent(
      `/api/sessions/${encodeURIComponent(sessionId)}/close`,
      { method: 'POST' },
    );
  }

  /** `GET /api/sessions/{id}/threads` — a session's thread tree, by creation. */
  getSessionThreads(sessionId: SessionId): Promise<ThreadsResponse> {
    return this.request<ThreadsResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/threads`,
    );
  }

  /**
   * `GET /api/sessions/{id}/sends` — a session's open (non-terminal) sends,
   * oldest first: status `queued` (held until the session goes idle) or
   * `dispatched` (typed into the pane, awaiting transcript correlation). The
   * server-side truth behind the pending-send strip. An unknown id is a `404`
   * (e.g. a reaped spawn), surfaced as {@link ApiError}.
   */
  getSessionSends(sessionId: SessionId): Promise<SendsResponse> {
    return this.request<SendsResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/sends`,
    );
  }

  /** `GET /api/threads/{id}/messages` — a thread's messages, ordered by seq. */
  getThreadMessages(threadId: ThreadId): Promise<MessagesResponse> {
    return this.request<MessagesResponse>(
      `/api/threads/${threadId}/messages`,
    );
  }

  /**
   * `POST /api/sends` — enqueue a send against an existing thread or spawn a new
   * session. The body is the discriminated {@link SendRequest} target; a branch
   * send sets `semantic_parent_uuid` on a {@link SendToThread} target.
   */
  createSend(body: SendRequest): Promise<SendResponse> {
    // The discriminated SendRequest narrows the flat wire shape; this
    // annotation keeps the narrowing assignable to the generated contract.
    const wireBody: CreateSendRequest = body;
    return this.request<SendResponse>('/api/sends', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(wireBody),
    });
  }

  /**
   * `POST /api/permissions/{id}/decision` — answer a pending tool-permission
   * request (204). The decision wakes the blocked `PermissionRequest` hook,
   * so the tool proceeds (or is denied) without touching the TUI prompt. A
   * `409` with code `permission_not_pending` means no browser decision can
   * reach the request anymore, surfaced as {@link ApiError}.
   */
  decidePermission(
    requestId: number,
    decision: PermissionDecision,
  ): Promise<void> {
    const body: PermissionDecisionRequest = { decision };
    return this.requestNoContent(
      `/api/permissions/${requestId}/decision`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    );
  }

  /**
   * `POST /api/sessions/{id}/questions/{requestId}/answer` — answer a pending
   * `AskUserQuestion` (204). The server turns the per-question selected option
   * indices into the exact TUI keystrokes and injects them into the session's
   * pane; the card then clears when the resulting `tool_result` resolves the
   * question's request row. A `409` (`question_not_pending`) means the question
   * can no longer be answered from the UI, and a `400` that the selection was
   * malformed — both surface as {@link ApiError}, and the card keeps its
   * answer-in-the-terminal fallback. `selections[q]` lists the chosen 0-based
   * option indices for question `q`.
   */
  answerQuestion(
    sessionId: SessionId,
    requestId: number,
    selections: number[][],
  ): Promise<void> {
    const body: QuestionAnswerRequest = { selections };
    return this.requestNoContent(
      `/api/sessions/${sessionId}/questions/${requestId}/answer`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    );
  }

  /**
   * `GET /api/workdir/list` — one level of a directory browse for the
   * new-session working-directory picker: the listed `path`, its `parent`
   * (`null` at a filesystem root), and its immediate subdirectories. Omitting
   * `path` lists `$HOME`. A missing/non-directory path is `400`, a permission
   * denial `403` — both surface as {@link ApiError}.
   */
  getWorkdirList(path?: string): Promise<WorkdirListResponse> {
    // `request()` has no query-string helper, so build it manually with the
    // same encodeURIComponent style as `getSessions`.
    const query: string[] = [];
    if (path !== undefined) {
      query.push(`path=${encodeURIComponent(path)}`);
    }
    const suffix = query.length > 0 ? `?${query.join('&')}` : '';
    return this.request<WorkdirListResponse>(`/api/workdir/list${suffix}`);
  }

  /**
   * `GET /api/workdir/recent` — recently-used working directories, most-recent
   * first, for the new-session picker's "Recent" list.
   */
  getWorkdirRecent(): Promise<WorkdirRecentResponse> {
    return this.request<WorkdirRecentResponse>('/api/workdir/recent');
  }
}
