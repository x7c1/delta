import type { SessionId, ThreadId } from '@delta/model';
import type {
  CloneRepositoryRequest,
  CreateLaunchOptionRequest,
  CreateCloneRootRequest,
  CreatePromptTemplateRequest,
  CreateSendRequest,
  GitBranchesResponse,
  GitRepoResponse,
  LaunchOption,
  LaunchOptionsResponse,
  MessagesResponse,
  NewSessionResponse,
  OpenCwdRequest,
  PermissionDecision,
  PermissionDecisionRequest,
  PromptTemplate,
  PromptTemplatesResponse,
  ProvidersResponse,
  PullRequestsResponse,
  QuestionAnswerRequest,
  QuestionCancelRequest,
  RepositoriesResponse,
  CloneRoot,
  CloneRootsResponse,
  SendRequest,
  SendResponse,
  SendsResponse,
  SessionsResponse,
  ThreadsResponse,
  UpdateLaunchOptionRequest,
  UpdatePromptTemplateRequest,
  VersionResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/wire-gen';

/** The two PR-list lenses backed by the `gh search`-powered endpoint. */
export type PullRequestLens = 'reviewer' | 'author';

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
 * Allow/Deny buttons for guidance chosen by the provider's `has_terminal`
 * capability.
 *
 * `permission_decision_unsupported` means the request is still pending but this
 * session's provider has no meaning for the decision value sent — today, a
 * session-scoped allow (`allow_for_session`) against a provider whose
 * `has_allow_for_session` capability is false. Nothing was mutated, so callers
 * drop the control that produced it and leave the remaining decisions usable,
 * rather than falling back as they do for `permission_not_pending`.
 *
 * `question_not_pending` means an `AskUserQuestion` can no longer be answered
 * from the UI (already answered, its turn ended, or no live pane). Callers
 * branch on this to keep the answer-in-the-terminal fallback.
 *
 * `send_not_cancellable` means a send can no longer be cancelled: it never
 * existed, is already terminal (matched a transcript line, or already
 * cancelled), or is dispatched but its echo already arrived so the in-flight
 * turn owns it. Callers may surface the refusal to the user (e.g.
 * `PendingQueue` routes it through `onError` to the notification store) and
 * let the pending strip reconcile from the next refetch.
 *
 * `send_not_releasable` means a send is not awaiting a release: it never
 * existed, was never restored by the server's boot-time reconcile, was
 * already released, or has since been cancelled. Callers surface the refusal
 * like a refused cancel and let the pending strip reconcile from the next
 * refetch.
 *
 * `clone_root_duplicate` means a clone root was registered twice with the same
 * path. The Settings dialog shows an inline "already registered" hint on this
 * code instead of a generic failure toast.
 *
 * `clone_root_not_registered` means a clone was requested into a directory that
 * is not one of the registered clone roots — the root must be spelled exactly as
 * `GET /api/clone-roots` returns it, so a path that was only just typed has to
 * be registered first and cloned under the spelling that registration returned.
 *
 * `clone_dest_exists` means `<clone_root>/<repo_name>` already exists, so the
 * clone was refused with no job started (there is no fallback naming, so the way
 * past it is a different clone root).
 *
 * Both surface inline on the PR row that asked for the clone, showing the
 * server's own message; the codes keep the two cases apart from a generic
 * failure.
 */
export type ApiErrorCode =
  | 'resume_unavailable'
  | 'permission_not_pending'
  | 'permission_decision_unsupported'
  | 'question_not_pending'
  | 'send_not_cancellable'
  | 'send_not_releasable'
  | 'clone_root_duplicate'
  | 'clone_root_not_registered'
  | 'clone_dest_exists'
  | 'open_cwd_path_not_allowed'
  | 'open_cwd_unknown_handler'
  | 'open_cwd_command_not_found'
  | 'open_cwd_spawn_failed';

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
   * `dispatched` (typed into the pane, awaiting transcript correlation). A
   * queued row with a non-null `restored_at` was recovered at the server's
   * boot from a dead process's `dispatched` state and never auto-dispatches
   * — it waits for an explicit {@link releaseSend} or {@link cancelSend}.
   * The server-side truth behind the pending-send strip. An unknown id is a
   * `404` (e.g. a reaped spawn), surfaced as {@link ApiError}.
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
   * `POST /api/sends/{id}/cancel` — cancel a `queued` or `dispatched` send
   * (204). The row flips to `cancelled` and drops out of the open-send list;
   * a dispatched send the turn machine is awaiting is discarded with an
   * `Escape` injection into the pane. A `409` (`send_not_cancellable`) fires
   * only when the send never existed, is already terminal (matched or
   * cancelled), or its echo already arrived (the in-flight turn owns it) —
   * surfaced as {@link ApiError} for the caller to show before the pending
   * strip reconciles from a refetch.
   */
  cancelSend(sendId: number): Promise<void> {
    return this.requestNoContent(`/api/sends/${sendId}/cancel`, {
      method: 'POST',
    });
  }

  /**
   * `POST /api/sends/{id}/release` — release a *restored* send into the
   * normal queued flow (204). A restored send (its `restored_at` is
   * non-null) was recovered at the server's boot from a `dispatched` state a
   * dead process left behind, and never auto-dispatches; this is the
   * explicit Send action on such a row. The server first ensures the owning
   * session is open — resuming it when it is closed, the normal state right
   * after the restart that created the row — so a release never strands the
   * send in a session nothing reopens. On success the marker clears and the
   * send dispatches through the normal queued path: immediately when the
   * session was already open and idle, or once the just-resumed session
   * settles (a `send_dispatched` event follows either way); mid-turn it
   * waits for the turn end. A `409` (`send_not_releasable`) fires when the
   * send never existed, was never restored, is already released, or has
   * since been cancelled; a failed resume surfaces its own error (e.g.
   * `409` `resume_unavailable`) with the marker untouched, so the release
   * can be retried — each surfaced as {@link ApiError} for the caller to
   * show before the pending strip reconciles from a refetch.
   */
  releaseSend(sendId: number): Promise<void> {
    return this.requestNoContent(`/api/sends/${sendId}/release`, {
      method: 'POST',
    });
  }

  /**
   * `POST /api/open-cwd` — launch an external tool (currently only VS Code)
   * against a session's cwd. The request `path` must be a path Delta has
   * already surfaced (a `session.cwd`, `session.requested_workdir`, or
   * `message.cwd`); the server rejects anything else with a `400`
   * (`open_cwd_path_not_allowed`) so a hand-crafted request cannot point the
   * editor at an arbitrary directory. `handler` selects which tool to
   * launch; omit for the default (`vscode`). A `500` may report a stable
   * code — `open_cwd_command_not_found` when the tool binary is missing
   * from `PATH`, or `open_cwd_spawn_failed` for any other spawn error.
   * Success is `204 No Content`; the browser shows no toast on success.
   */
  openCwd(body: OpenCwdRequest): Promise<void> {
    return this.requestNoContent('/api/open-cwd', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `POST /api/permissions/{id}/decision` — answer a pending tool-permission
   * request (204). The decision wakes the blocked `PermissionRequest` hook,
   * so the tool proceeds (or is denied) without touching the TUI prompt. A
   * `409` with code `permission_not_pending` means no browser decision can
   * reach the request anymore, surfaced as {@link ApiError}.
   *
   * `'allow_for_session'` is **not** accepted by every provider: only one whose
   * `ProviderCapabilities.has_allow_for_session` is `true`. Sending it to any
   * other is a `400` with code `permission_decision_unsupported` — nothing is
   * mutated and the request stays answerable with `'allow'` or `'deny'`, so a
   * caller offering that choice gates it on the capability and treats the `400`
   * as "retire that option", not as a dead request.
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
   * `POST /api/sessions/{id}/questions/cancel` — cancel a pending
   * `AskUserQuestion` (204). The sibling of {@link answerQuestion}: the server
   * injects a single `Escape` into the session's pane, which cancels the whole
   * call; the card then clears when the resulting `is_error` `tool_result`
   * resolves the question's request row. A `409` (`question_not_pending`) means
   * the question can no longer be cancelled from the UI — surfaced as
   * {@link ApiError}, and the card keeps its cancel-in-the-terminal fallback.
   * Cancel carries no selection, so `requestId` rides in the body.
   */
  cancelQuestion(sessionId: SessionId, requestId: number): Promise<void> {
    const body: QuestionCancelRequest = { request_id: requestId };
    return this.requestNoContent(
      `/api/sessions/${sessionId}/questions/cancel`,
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

  /**
   * `GET /api/repositories` — registered repositories for the new-session
   * Repository tab, most-recently-active first. Each entry bundles its
   * known clones (one per `(repo_root, requested_workdir)` pair) under a
   * single identity key derived from the repo's `origin` URL. Clones
   * whose path no longer exists on disk are filtered out server-side.
   */
  getRepositories(): Promise<RepositoriesResponse> {
    return this.request<RepositoriesResponse>('/api/repositories');
  }

  /**
   * `GET /api/prs?lens=…` — open pull requests for the new-session PR
   * tab, one of two lenses:
   *
   * - `reviewer` — open PRs that requested the authenticated user's
   *   review, drafts excluded;
   * - `author` — open PRs the authenticated user authored, drafts
   *   included.
   *
   * The server reports `gh_available: false` when the `gh` CLI is not
   * installed or `gh auth status` fails, so a host without gh still
   * returns 200 with an empty list — the PR tab renders an inline
   * "run `gh auth login`" hint rather than treating it as an error.
   * Each row carries `has_local_clone` derived by the use case so the
   * UI can gate the click → composer pre-fill.
   */
  getPullRequests(lens: PullRequestLens): Promise<PullRequestsResponse> {
    return this.request<PullRequestsResponse>(
      `/api/prs?lens=${encodeURIComponent(lens)}`,
    );
  }

  /**
   * `GET /api/workdir/git` — whether a directory is inside a git repository.
   * `repo_root` is `null` when it is not, and `default_branch` carries the
   * repository's default branch short name when known. Computed without any
   * network access, so it is cheap enough to run as soon as a directory is
   * selected (used to decide whether to offer the worktree option). The picker
   * builds the query string with the same `encodeURIComponent` style as
   * {@link getWorkdirList}.
   *
   * A non-git path is NOT an error — it answers `repo_root: null`. The one
   * failure is a blank `path` (the server trims before checking), which is a
   * `400` surfaced as {@link ApiError}.
   */
  getGitRepoInfo(path: string): Promise<GitRepoResponse> {
    return this.request<GitRepoResponse>(
      `/api/workdir/git?path=${encodeURIComponent(path)}`,
    );
  }

  /**
   * `GET /api/workdir/git/branches` — the remote branches of the repository
   * containing `path`, reflecting a fresh `git fetch` (so it is slow-ish; call
   * it lazily, only when the user opens the remote-branch picker). A non-git
   * path is a `400`, and so is a blank one (the server trims before checking);
   * both are surfaced as {@link ApiError}.
   */
  getGitBranches(path: string): Promise<GitBranchesResponse> {
    return this.request<GitBranchesResponse>(
      `/api/workdir/git/branches?path=${encodeURIComponent(path)}`,
    );
  }

  /**
   * `GET /api/launch-options` — the registered custom launch options (`claude`
   * CLI flag records), newest first, for the settings screen to manage.
   */
  getLaunchOptions(): Promise<LaunchOptionsResponse> {
    return this.request<LaunchOptionsResponse>('/api/launch-options');
  }

  /**
   * `POST /api/launch-options` — register a custom launch option. `name` (the
   * flag) is required; `label` and `value` are optional. A blank `name` is a
   * `400`, surfaced as {@link ApiError}. Returns the created record.
   */
  createLaunchOption(
    body: CreateLaunchOptionRequest,
  ): Promise<LaunchOption> {
    return this.request<LaunchOption>('/api/launch-options', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `PATCH /api/launch-options/{id}` — set a launch option's `default_enabled`
   * flag in place (id and `created_at` are preserved). An unknown id is a `404`,
   * surfaced as {@link ApiError}. Returns the updated record.
   */
  updateLaunchOption(
    id: number,
    body: UpdateLaunchOptionRequest,
  ): Promise<LaunchOption> {
    return this.request<LaunchOption>(`/api/launch-options/${id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `DELETE /api/launch-options/{id}` — remove a registered launch option
   * (204). Deleting an unknown id is a no-op, so this is idempotent.
   */
  deleteLaunchOption(id: number): Promise<void> {
    return this.requestNoContent(`/api/launch-options/${id}`, {
      method: 'DELETE',
    });
  }

  /**
   * `GET /api/prompt-templates` — the registered prompt templates (named blocks
   * of instruction text for the composer), oldest first. Global rather than
   * provider-scoped: the text is prose, so the same template applies to every
   * provider.
   */
  getPromptTemplates(): Promise<PromptTemplatesResponse> {
    return this.request<PromptTemplatesResponse>('/api/prompt-templates');
  }

  /**
   * `POST /api/prompt-templates` — register a prompt template. `label` and
   * `text` are both required and must be non-blank once trimmed; a blank one is
   * a `400`, surfaced as {@link ApiError}. `text` is stored verbatim, so its own
   * leading/trailing newlines survive. Returns the created record.
   */
  createPromptTemplate(
    body: CreatePromptTemplateRequest,
  ): Promise<PromptTemplate> {
    return this.request<PromptTemplate>('/api/prompt-templates', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `PATCH /api/prompt-templates/{id}` — replace a template's `label` and
   * `text` in place (id and `created_at` are preserved, `updated_at` is
   * re-stamped). Both fields are required: this is a full replacement of the
   * editable content, not a partial patch. A blank field is a `400` and an
   * unknown id a `404`, both surfaced as {@link ApiError}. Returns the updated
   * record.
   */
  updatePromptTemplate(
    id: number,
    body: UpdatePromptTemplateRequest,
  ): Promise<PromptTemplate> {
    return this.request<PromptTemplate>(`/api/prompt-templates/${id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `DELETE /api/prompt-templates/{id}` — remove a registered prompt template
   * (204). Deleting an unknown id is a no-op, so this is idempotent.
   */
  deletePromptTemplate(id: number): Promise<void> {
    return this.requestNoContent(`/api/prompt-templates/${id}`, {
      method: 'DELETE',
    });
  }

  /**
   * `POST /api/repositories/clone` — clone a repository the user has no local
   * clone of into a registered clone root (`202`, no body).
   *
   * The clone runs as a background job on the server: the outcome arrives on
   * `/ws` as `repository_clone_completed` / `repository_clone_failed`, never in
   * this response. Requesting a destination that is already being cloned joins
   * that job rather than starting a second one, so a double-click is harmless.
   *
   * An unregistered `clone_root` is a `400` with code
   * `clone_root_not_registered`; an existing `<clone_root>/<repo_name>` is a
   * `409` with code `clone_dest_exists`. Both surface as {@link ApiError}.
   */
  cloneRepository(body: CloneRepositoryRequest): Promise<void> {
    return this.requestNoContent('/api/repositories/clone', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `GET /api/clone-roots` — registered clone roots, newest first. Each clone
   * root is a directory where the user's git clones live; the Repository tab
   * probes its direct children for clones on every refetch, surfacing clones
   * the user has not yet started a session in.
   */
  getCloneRoots(): Promise<CloneRootsResponse> {
    return this.request<CloneRootsResponse>('/api/clone-roots');
  }

  /**
   * `POST /api/clone-roots` — register a new clone root. `path` must be a
   * non-blank absolute path. The server trims a trailing slash. A duplicate
   * path is a `409` with code `clone_root_duplicate`, surfaced as
   * {@link ApiError} so the Settings dialog can show an inline hint.
   */
  createCloneRoot(body: CreateCloneRootRequest): Promise<CloneRoot> {
    return this.request<CloneRoot>('/api/clone-roots', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }

  /**
   * `DELETE /api/clone-roots/{path_b64}` — unregister a clone root
   * (204). The registered path is URL-safe base64-encoded into the path
   * segment so its embedded `/` characters survive routing. Deleting an
   * unknown path is a no-op, so this is idempotent.
   */
  deleteCloneRoot(path: string): Promise<void> {
    return this.requestNoContent(`/api/clone-roots/${encodeBase64Url(path)}`, {
      method: 'DELETE',
    });
  }

  /**
   * `GET /api/version` — the Delta workspace version string for the navigator
   * footer. Pre-formatted server-side (`v0.2.1` or, on debug builds,
   * `v0.2.1+dev.<sha>`), so the browser renders it verbatim.
   */
  getVersion(): Promise<VersionResponse> {
    return this.request<VersionResponse>('/api/version');
  }

  /**
   * `GET /api/providers` — per-provider launch availability for the new-session
   * selector. Each entry reports whether that provider's launch binary is
   * present on the server host (`available`), with a `detail` reason when it is
   * not. Always 200: a missing binary is reported in-band as `available: false`,
   * never as an error.
   */
  getProviders(): Promise<ProvidersResponse> {
    return this.request<ProvidersResponse>('/api/providers');
  }
}

/**
 * Encode `value` as URL-safe base64 (RFC 4648 §5), no padding. Used to wrap
 * the registered clone-root path in the DELETE path segment without `%2F`
 * escaping its embedded slashes. The implementation is small enough to inline
 * rather than pull in a dependency.
 */
function encodeBase64Url(value: string): string {
  // `btoa` only accepts Latin-1; encode the UTF-8 bytes first, then re-decode
  // each as a Latin-1 code point so `btoa` reads exactly the original bytes.
  const bytes = new TextEncoder().encode(value);
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}
