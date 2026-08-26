//! Browser REST surface.
//!
//! The browser talks to the server over a REST + WebSocket hybrid: queries and
//! commands go through these `/api/*` routes (so they are easy to mock on the
//! frontend), live deltas arrive over `/ws`, and the terminal is bridged over
//! `/pty`. Every handler maps onto the use-case [`Interactor`]; errors are
//! converted to HTTP responses through a single [`ApiError`] mapping.
//!
//! The surface is multi-session: sessions are listed, created, opened, and
//! closed by id, and threads and sends are routed to a specific session rather
//! than an implicit "current" one.
//!
//! The JSON shapes themselves live in the `delta_wire` crate (its [`rest`]
//! module), which also generates the frontend's TypeScript bindings. Handlers
//! convert at this boundary: domain values in and out of the use cases, wire
//! types on the HTTP surface.
//!
//! [`Interactor`]: delta_usecase::Interactor
//! [`rest`]: delta_wire::rest

mod api_error;
pub(crate) use api_error::ApiError;
pub(crate) mod clone_root_path;
mod session_cursor;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use delta_usecase::{AgentProvider, PullRequestLens, SessionId, ThreadId};
use delta_wire::rest::{
    WireCloneRepositoryRequest, WireCloneRoot, WireCloneRootsResponse, WireCreateCloneRootRequest,
    WireCreateLaunchOptionRequest, WireCreatePromptTemplateRequest, WireCreateSendRequest,
    WireGitBranchesResponse, WireGitRepoResponse, WireLaunchOption, WireLaunchOptionsResponse,
    WireMessagesResponse, WireNewSessionResponse, WireOpenCwdRequest,
    WirePermissionDecisionRequest, WirePromptTemplate, WirePromptTemplatesResponse,
    WireProvidersResponse, WirePullRequestsResponse, WireQuestionAnswerRequest,
    WireQuestionCancelRequest, WireRecentWorkdirItem, WireRepositoriesResponse,
    WireRepositoryEntry, WireSendResponse, WireSendsResponse, WireSessionListItem,
    WireSessionsResponse, WireThreadsResponse, WireUpdateLaunchOptionRequest,
    WireUpdatePromptTemplateRequest, WireVersionResponse, WireWorkdirListResponse,
    WireWorkdirRecentResponse,
};

use crate::state::AppState;

/// The default page size when the request omits `limit`.
const DEFAULT_PAGE_LIMIT: u32 = 30;

/// The hard cap on page size, so a caller cannot ask for an unbounded page.
const MAX_PAGE_LIMIT: u32 = 100;

/// Query parameters for `GET /api/sessions`: the opaque page cursor and an
/// optional page-size override. Mirrors the local-struct convention used by the
/// PTY bridge's `PtyQuery`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListSessionsQuery {
    /// The `next_cursor` echoed back from the previous page, or absent for the
    /// first page. Opaque: encoded/decoded by [`session_cursor`].
    cursor: Option<String>,
    /// Requested page size, clamped to `[1, MAX_PAGE_LIMIT]`; defaults to
    /// `DEFAULT_PAGE_LIMIT` when absent.
    limit: Option<u32>,
}

/// `GET /api/sessions` — one page of known sessions, most-recently-active first.
///
/// Returns a single page (most-recently-active first), each session tagged with
/// whether it currently has a live pane (`open`) and its `main` thread id, so
/// the navigator can show and route into every conversation — open or closed.
/// The page size is `limit` (default [`DEFAULT_PAGE_LIMIT`], capped at
/// [`MAX_PAGE_LIMIT`]). When more rows may follow, `next_cursor` carries an
/// opaque token the caller echoes back as `cursor` to fetch the next page; a
/// malformed `cursor` is a `400`.
pub(crate) async fn list_sessions(
    State(state): State<AppState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<WireSessionsResponse>, ApiError> {
    let cursor = match query.cursor {
        Some(token) => Some(
            session_cursor::decode(&token)
                .ok_or_else(|| ApiError::BadRequest("malformed cursor".to_owned()))?,
        ),
        None => None,
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);

    let page = state.interactor().list_sessions_page(cursor, limit).await?;
    Ok(Json(WireSessionsResponse {
        sessions: page
            .listings
            .into_iter()
            .map(WireSessionListItem::from)
            .collect(),
        next_cursor: page.next.as_ref().map(session_cursor::encode),
    }))
}

/// `POST /api/sessions` — spawn a fresh session eagerly.
///
/// Used by cold start (an empty session list) and the "New" button. Returns the
/// tmux/process lifecycle so the UI can show a "starting" indicator until the
/// session is usable. The conversational session is still registered later by
/// the first `UserPromptSubmit` hook, but the row is written before the launch,
/// so the session is listed by `GET /api/sessions` straight away with
/// `status: "spawning"` and flips to `active` at registration.
pub(crate) async fn create_session(
    State(state): State<AppState>,
) -> Result<Json<WireNewSessionResponse>, ApiError> {
    let status = state.ensure_session().await?;
    Ok(Json(WireNewSessionResponse::from(status)))
}

/// `POST /api/sessions/{id}/open` — resume a closed, known session.
///
/// For a Claude session this re-launches `claude --resume <id>` and binds the
/// new pane; for a terminal-less Codex session it reconnects the adapter via
/// `thread/resume` (there is no pane). Either way it broadcasts `SessionOpened`,
/// and re-opening an already-open session is a no-op.
pub(crate) async fn open_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    state.interactor().open_session(&id).await?;
    state.broadcast([delta_usecase::SessionEvent::SessionOpened { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/close` — tear down a session's pane, keep its data.
///
/// Kills the live pane and drops it from the registry, broadcasting
/// `SessionClosed`; the conversation remains in the store and can be reopened.
/// Closing also sweeps any lingering background subagent whose completion
/// notification can no longer arrive; the resulting `SubagentFinished` events
/// are broadcast so live viewers' indicators clear immediately.
pub(crate) async fn close_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    let subagent_finished = state.interactor().close_session(&id).await?;
    state.broadcast(subagent_finished);
    state.broadcast([delta_usecase::SessionEvent::SessionClosed { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/interrupt` — abort a session's in-flight turn.
///
/// For a terminal-less agent (Codex) this drives the adapter's `interrupt`
/// without closing the session; the resulting `TurnInterrupted` settles over
/// the async event seam (the WebSocket broadcast), so — like a permission
/// decision or a question answer on a Codex session — no event is broadcast
/// synchronously here. For a pane-backed (Claude) or closed session it is a
/// well-defined no-op: Claude's turn interrupt is TUI-driven (Escape in the
/// pane).
pub(crate) async fn interrupt(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.interactor().interrupt(&SessionId::from(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/sessions/{id}/threads` — a session's thread tree for the navigator.
pub(crate) async fn list_threads(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WireThreadsResponse>, ApiError> {
    let threads = state.interactor().threads_for(&SessionId::from(id)).await?;
    Ok(Json(WireThreadsResponse::from(threads)))
}

/// `GET /api/sessions/{id}/sends` — a session's open (non-terminal) sends.
///
/// Returns the sends still in flight for the session — status `queued`
/// (held back until the session goes idle) or `dispatched` (typed into the
/// pane, awaiting transcript correlation) — oldest first. A queued row may
/// carry `held_at`: it was recovered at boot from a dead process's
/// `dispatched` state and never auto-dispatches — the browser renders it
/// with explicit Send ([`release_send`]) and Cancel actions instead of the
/// waiting label. This is the source of truth for the browser's send strip.
/// An unknown session id is a `404`, so a reaped spawn is distinguishable
/// from "nothing pending".
pub(crate) async fn list_sends(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WireSendsResponse>, ApiError> {
    let id = SessionId::from(id);
    let sends = state.interactor().open_sends_for(&id).await?;
    // The queryable live state (turn phase + pending permission dialog) rides
    // along so a reconnecting client can rebuild its in-progress indicator and
    // its permission notice from this one refetch (events broadcast while the
    // socket was down are not replayed).
    let live = state.interactor().live_state_for(&id).await;
    Ok(Json(WireSendsResponse::new(sends, live)))
}

/// `GET /api/threads/{id}/messages` — a thread's messages for drill-down.
///
/// Thread ids are globally unique, so this is not scoped by session.
pub(crate) async fn thread_messages(
    State(state): State<AppState>,
    Path(thread_id): Path<i64>,
) -> Result<Json<WireMessagesResponse>, ApiError> {
    let messages = state.interactor().thread_view(ThreadId(thread_id)).await?;
    Ok(Json(WireMessagesResponse::from(messages)))
}

/// Query parameters for `GET /api/workdir/list`: the directory to browse.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkdirListQuery {
    /// The absolute path to list. Omitted or empty defaults to the user's home
    /// directory, so the picker has a sensible starting point.
    #[serde(default)]
    path: Option<String>,
}

/// `GET /api/workdir/list` — browse a directory for the working-directory picker.
///
/// Lists the immediate subdirectories of `path` (dirs only, dot-directories
/// hidden, sorted by name), along with the canonical path and its parent so the
/// picker can step up. `path` defaults to `$HOME` when omitted. A missing path
/// or a non-directory is a `400`; a permission error is a `403`.
pub(crate) async fn list_workdir(
    State(state): State<AppState>,
    Query(query): Query<WorkdirListQuery>,
) -> Result<Json<WireWorkdirListResponse>, ApiError> {
    let listing = state
        .interactor()
        .browse_workdir(query.path.as_deref())
        .await?;
    Ok(Json(WireWorkdirListResponse::from(listing)))
}

/// `GET /api/workdir/recent` — recently-used working directories for the picker.
///
/// Returns the distinct directories sessions have run in, most-recently-used
/// first, derived from existing session rows (Delta keeps no separate history).
pub(crate) async fn recent_workdir(
    State(state): State<AppState>,
) -> Result<Json<WireWorkdirRecentResponse>, ApiError> {
    let workdirs = state.interactor().recent_workdirs().await?;
    Ok(Json(WireWorkdirRecentResponse {
        workdirs: workdirs
            .into_iter()
            .map(WireRecentWorkdirItem::from)
            .collect(),
    }))
}

/// `GET /api/repositories` — registered repositories for the new-session
/// Repository tab, ordered by the most recent activity across each
/// repository's clones.
///
/// Aggregates the session history: every distinct (repo_root, clone_path)
/// pair becomes a clone, and clones whose `git config --get
/// remote.origin.url` collapses to the same normalised key bundle under one
/// repository. Clones whose path no longer exists on disk are filtered out
/// (lazy GC); a repository drained of every clone disappears too. Sessions
/// launched outside any git repo do not contribute — the Recent dirs list
/// (Directory tab) is where those surface.
pub(crate) async fn list_repositories(
    State(state): State<AppState>,
) -> Result<Json<WireRepositoriesResponse>, ApiError> {
    let repositories = state.interactor().list_repositories().await?;
    Ok(Json(WireRepositoriesResponse {
        repositories: repositories
            .into_iter()
            .map(WireRepositoryEntry::from)
            .collect(),
    }))
}

/// `POST /api/repositories/clone` — clone a repository into a registered clone
/// root.
///
/// Accepts (`202`) and runs the clone as a background job: cloning takes far
/// longer than a request should, so the outcome arrives on `/ws` as
/// `repository_clone_completed` / `repository_clone_failed` rather than in this
/// response. The refusals happen here, before any job exists: an unregistered
/// `clone_root` is a `400` with code `clone_root_not_registered`, and an
/// already-occupied `<clone_root>/<repo_name>` is a `409` with code
/// `clone_dest_exists` — there is no fallback naming, so Delta refuses rather
/// than cloning next to it. A second request for a destination already being
/// cloned joins that job (also `202`) instead of starting a second `gh`.
pub(crate) async fn clone_repository(
    State(state): State<AppState>,
    Json(req): Json<WireCloneRepositoryRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .interactor()
        .clone_repository(&req.repo_owner, &req.repo_name, &req.clone_root)
        .await?;
    Ok(StatusCode::ACCEPTED)
}

/// `GET /api/clone-roots` — the registered clone roots.
///
/// Returns the directories the user has registered as homes for their git
/// clones, newest first. Each entry carries only the path; the stored
/// `created_at` is omitted from the wire because the Settings list does not
/// show it.
pub(crate) async fn list_clone_roots(
    State(state): State<AppState>,
) -> Result<Json<WireCloneRootsResponse>, ApiError> {
    let roots = state.interactor().list_clone_roots().await?;
    Ok(Json(WireCloneRootsResponse {
        clone_roots: roots.into_iter().map(WireCloneRoot::from).collect(),
    }))
}

/// `POST /api/clone-roots` — register a new clone root.
///
/// `path` must be a non-blank absolute path (starting with `/`). Trailing
/// slashes are trimmed for canonicalisation, so `/home/dev/projects/` and
/// `/home/dev/projects` register the same row. Blank input — empty,
/// whitespace-only, or nothing but slashes — is a `400`; the bare root `/` is
/// not blank and registers like any other path. The path is NOT required to
/// exist or to contain git repos at registration time — a future-state clone
/// root is allowed. Duplicate paths return `409` with code
/// `clone_root_duplicate` so the Settings dialog can show an inline hint.
pub(crate) async fn create_clone_root(
    State(state): State<AppState>,
    Json(req): Json<WireCreateCloneRootRequest>,
) -> Result<(StatusCode, Json<WireCloneRoot>), ApiError> {
    let trimmed = req.path.trim();
    // Strip trailing slashes so the user-typed form is canonicalised before the
    // PRIMARY KEY check sees it. Only the bare `/` is exempt: it is the one
    // all-slash spelling this contract takes as a deliberate root, while `"//"`
    // and `"///"` strip down to nothing, blank like `""` and `"   "`.
    // Canonicalising before the blankness check below is what rejects those
    // instead of quietly registering `/`.
    let canonical = if trimmed == "/" {
        "/"
    } else {
        trimmed.trim_end_matches('/')
    };
    if canonical.is_empty() {
        return Err(ApiError::BadRequest(
            "a clone root must have a non-blank `path`".to_owned(),
        ));
    }
    if !canonical.starts_with('/') {
        return Err(ApiError::BadRequest(
            "a clone root `path` must be absolute (start with `/`)".to_owned(),
        ));
    }
    let root = state.interactor().add_clone_root(canonical).await?;
    Ok((StatusCode::CREATED, Json(WireCloneRoot::from(root))))
}

/// `DELETE /api/clone-roots/{path_b64}` — unregister a clone root.
///
/// The registered absolute path is URL-safe base64 in the path segment to
/// keep its embedded `/` characters out of the route match. A malformed token
/// is a `400`; an unknown path is a silent no-op (idempotent), so a
/// Settings dialog click never surfaces a 404 noise on a path the user just
/// removed via another tab.
pub(crate) async fn delete_clone_root(
    State(state): State<AppState>,
    Path(path_b64): Path<String>,
) -> Result<StatusCode, ApiError> {
    let path = clone_root_path::decode(&path_b64)
        .ok_or_else(|| ApiError::BadRequest("malformed clone-root path token".to_owned()))?;
    state.interactor().remove_clone_root(&path).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Query parameters for `GET /api/prs`: which lens to query gh for.
#[derive(Debug, Deserialize)]
pub(crate) struct ListPullRequestsQuery {
    /// The lens name (`reviewer` or `author`). Required: there is no
    /// sensible default lens — the PR tab asks for one explicitly per
    /// section.
    lens: String,
}

/// `GET /api/prs?lens=reviewer|author` — pull requests for the new-session
/// PR tab.
///
/// Drives the PR search through the gateway, then joins the result
/// against the registered repositories so each row carries
/// `has_local_clone`. When `gh` is not installed or `gh auth status`
/// fails, the response is `{ gh_available: false, pull_requests: [] }`
/// at 200 — the PR tab renders an inline "run `gh auth login`" hint
/// rather than a generic failure. An unknown `lens` is a `400`.
pub(crate) async fn list_pull_requests(
    State(state): State<AppState>,
    Query(query): Query<ListPullRequestsQuery>,
) -> Result<Json<WirePullRequestsResponse>, ApiError> {
    let lens = PullRequestLens::parse(&query.lens).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "unknown lens '{}': expected 'reviewer' or 'author'",
            query.lens
        ))
    })?;
    let list = state.interactor().list_pull_requests(lens).await?;
    Ok(Json(WirePullRequestsResponse::from(list)))
}

/// `GET /api/providers` — launch availability and capability profile for every
/// known agent provider.
///
/// For each provider (Claude, Codex) reports whether its configured launch
/// binary is present on the server host, with a reason string when it is not,
/// plus the provider's UI-relevant capability profile (e.g. whether it offers an
/// attachable terminal). The new-session provider selector disables an
/// unavailable provider and shows the reason, so a user cannot pick a provider
/// that would fail at spawn; the workspace reads the capability profile to gate
/// provider-specific surfaces (the terminal tab is hidden for a provider with no
/// terminal). Always `200`: a missing binary is data (`available: false`), never
/// an error.
pub(crate) async fn list_providers(State(state): State<AppState>) -> Json<WireProvidersResponse> {
    let availability = state.interactor().provider_availability().await;
    // Pair each provider's runtime launch availability with its static
    // capability profile. The profile comes from the composition root's
    // per-provider accessor (which reads the same const each adapter's
    // `capabilities()` returns), so it is resolved without a live adapter and
    // can never drift from what a running adapter reports.
    let entries = availability
        .into_iter()
        .map(|availability| {
            let capabilities = delta_bootstrap::provider_capabilities(availability.provider);
            (availability, capabilities)
        })
        .collect::<Vec<_>>();
    Json(WireProvidersResponse::from(entries))
}

/// Query parameters for the git-detection endpoints: the directory to inspect.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkdirGitQuery {
    /// The absolute path to inspect. Required: unlike the browse endpoints there
    /// is no sensible default repository to fall back to.
    #[serde(default)]
    path: Option<String>,
}

impl WorkdirGitQuery {
    /// The required `path` with surrounding whitespace trimmed, or a `400` when
    /// it is missing or blank. Nothing downstream trims it — the git gateway
    /// trims git's output, never the path it is handed — so the trim has to
    /// happen here for the callers to receive a usable path.
    fn require_path(&self) -> Result<&str, ApiError> {
        match self.path.as_deref().map(str::trim) {
            Some(path) if !path.is_empty() => Ok(path),
            _ => Err(ApiError::BadRequest(
                "a `path` query parameter is required".to_owned(),
            )),
        }
    }
}

/// `GET /api/workdir/git` — detect whether a directory is a git repository.
///
/// Returns `{ repo_root, default_branch }`: `repo_root` is the repository root
/// containing `path` (`null` when it is not inside a git repository), and
/// `default_branch` is that repository's default branch when known. No fetch, so
/// this is cheap to call as the picker's selection changes. A missing or blank
/// `path` is a `400`.
pub(crate) async fn workdir_git(
    State(state): State<AppState>,
    Query(query): Query<WorkdirGitQuery>,
) -> Result<Json<WireGitRepoResponse>, ApiError> {
    let path = query.require_path()?;
    let info = state.interactor().git_repo_info(path).await?;
    Ok(Json(WireGitRepoResponse::from(info)))
}

/// `GET /api/workdir/git/branches` — the remote branches of a repository.
///
/// Resolves the repository containing `path`, fetches the remote, and returns
/// `{ default_branch, remote_branches }` so a branch picker can offer a base for
/// a worktree. A `path` that is not inside a git repository is a `400` (the
/// `not a git repository` use-case error), and a missing or blank `path` is also
/// a `400`.
pub(crate) async fn workdir_git_branches(
    State(state): State<AppState>,
    Query(query): Query<WorkdirGitQuery>,
) -> Result<Json<WireGitBranchesResponse>, ApiError> {
    let path = query.require_path()?;
    let remote = state.interactor().git_remote_branches(path).await?;
    Ok(Json(WireGitBranchesResponse::from(remote)))
}

/// `GET /api/launch-options` — the registered launch options.
///
/// Returns the flat `(label?, name, value?)` records for the settings screen to
/// list and manage: the rows Delta ships first, in declared-catalog order, then
/// the ones the user registered, newest first. `builtin` tells the two apart.
/// Selecting which to apply when starting a session is a separate concern
/// handled elsewhere.
pub(crate) async fn list_launch_options(
    State(state): State<AppState>,
) -> Result<Json<WireLaunchOptionsResponse>, ApiError> {
    let options = state.interactor().list_launch_options().await?;
    Ok(Json(WireLaunchOptionsResponse {
        launch_options: options.into_iter().map(WireLaunchOption::from).collect(),
    }))
}

/// `POST /api/launch-options` — register a new custom launch option.
///
/// `name` is required and must be non-blank; `label` and `value` are optional
/// (a valueless option carries no `value`). A blank `name` is a `400`. What
/// `name` means is the provider's business — a CLI flag for Claude, a
/// `thread/start` field for Codex — so the validation here is deliberately only
/// "present and non-blank", and the message stays provider-neutral. Returns the
/// created record so the client can render it without a refetch.
pub(crate) async fn create_launch_option(
    State(state): State<AppState>,
    Json(req): Json<WireCreateLaunchOptionRequest>,
) -> Result<(StatusCode, Json<WireLaunchOption>), ApiError> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "a launch option must have a non-blank `name`".to_owned(),
        ));
    }
    // `label`/`value` are kept verbatim apart from trimming surrounding
    // whitespace; an all-blank optional is treated as absent rather than a
    // stored empty string.
    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let value = req
        .value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // A create that omits `provider` is a Claude option, keeping clients that
    // predate per-provider launch options working unchanged.
    let provider = req
        .provider
        .map(AgentProvider::from)
        .unwrap_or(AgentProvider::Claude);
    let option = state
        .interactor()
        .create_launch_option(label, name, value, req.default_enabled, provider)
        .await?;
    Ok((StatusCode::CREATED, Json(WireLaunchOption::from(option))))
}

/// `PATCH /api/launch-options/{id}` — set a launch option's `default_enabled`
/// flag in place.
///
/// Updating in place preserves the option's id and `created_at` (a
/// delete+recreate would churn both); `name`, `value`, and `label` are immutable
/// through this endpoint. Returns the updated record so the client can render it
/// without a refetch, or `404` when no option has that id.
///
/// Applies to a row Delta ships exactly as it does to the user's own. That the
/// three content fields are immutable *here* is precisely what lets startup
/// refresh a shipped row's `label`/`name`/`value` from the declared catalog
/// without ever overwriting something the user typed.
pub(crate) async fn update_launch_option(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WireUpdateLaunchOptionRequest>,
) -> Result<Json<WireLaunchOption>, ApiError> {
    let option = state
        .interactor()
        .set_launch_option_default_enabled(id, req.default_enabled)
        .await?;
    match option {
        Some(option) => Ok(Json(WireLaunchOption::from(option))),
        None => Err(ApiError::NotFound(format!("no launch option with id {id}"))),
    }
}

/// `DELETE /api/launch-options/{id}` — remove a registered launch option.
///
/// Deleting an unknown id is a no-op, so this is idempotent and replies `204`.
/// A row Delta ships is refused with a `409` (see
/// [`delta_usecase::Error::LaunchOptionIsBuiltin`]): the declared catalog owns
/// those, so a removed row would simply reappear at the next startup. `PATCH` on
/// the same row still works — ticking `default_enabled` on a shipped option is
/// the point of shipping it.
pub(crate) async fn delete_launch_option(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().delete_launch_option(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/prompt-templates` — the registered prompt templates.
///
/// Returns the `(label, text)` records the user registered as reusable composer
/// instructions, oldest first, for the settings screen to manage and the
/// composer to insert from. Unlike launch options the list is global: the text
/// is provider-independent prose, so nothing is filtered per provider.
pub(crate) async fn list_prompt_templates(
    State(state): State<AppState>,
) -> Result<Json<WirePromptTemplatesResponse>, ApiError> {
    let templates = state.interactor().list_prompt_templates().await?;
    Ok(Json(WirePromptTemplatesResponse {
        prompt_templates: templates
            .into_iter()
            .map(WirePromptTemplate::from)
            .collect(),
    }))
}

/// `POST /api/prompt-templates` — register a new prompt template.
///
/// Both `label` and `text` are required and must be non-blank; a blank one is a
/// `400` from the use case, which names the offending field. Both are stored
/// verbatim — the emptiness check trims, the storage does not — so a template
/// that ends with a newline keeps it. Returns the created record so the client
/// can render it without a refetch.
pub(crate) async fn create_prompt_template(
    State(state): State<AppState>,
    Json(req): Json<WireCreatePromptTemplateRequest>,
) -> Result<(StatusCode, Json<WirePromptTemplate>), ApiError> {
    let template = state
        .interactor()
        .create_prompt_template(&req.label, &req.text)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(WirePromptTemplate::from(template)),
    ))
}

/// `PATCH /api/prompt-templates/{id}` — replace a template's content in place.
///
/// Updating in place preserves the template's id and `created_at` (a
/// delete+recreate would churn both and move the row to the end of the list) and
/// re-stamps `updated_at`. Both fields are required, and held to the same
/// non-blank rule as the create (`400`). Returns the updated record, or `404`
/// when no template has that id.
pub(crate) async fn update_prompt_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WireUpdatePromptTemplateRequest>,
) -> Result<Json<WirePromptTemplate>, ApiError> {
    let template = state
        .interactor()
        .update_prompt_template(id, &req.label, &req.text)
        .await?;
    match template {
        Some(template) => Ok(Json(WirePromptTemplate::from(template))),
        None => Err(ApiError::NotFound(format!(
            "no prompt template with id {id}"
        ))),
    }
}

/// `DELETE /api/prompt-templates/{id}` — remove a registered prompt template.
///
/// Deleting an unknown id is a no-op, so this is idempotent and always replies
/// `204`.
pub(crate) async fn delete_prompt_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().delete_prompt_template(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sends` — enqueue a send into a session named by the request.
///
/// The session is derived from the target thread for an existing send, or
/// created for a `new_session` send. Turn confirmation arrives later via the
/// `UserPromptSubmit` hook; only enqueue-time events (e.g. `send_dispatched`
/// from the idle-flush) are broadcast here.
pub(crate) async fn create_send(
    State(state): State<AppState>,
    Json(req): Json<WireCreateSendRequest>,
) -> Result<(StatusCode, Json<WireSendResponse>), ApiError> {
    let (target, text, locator_quote) = req
        .into_target()
        .map_err(|err| ApiError::BadRequest(err.message().to_owned()))?;
    let (send, events) = state
        .interactor()
        .enqueue_send(target, &text, locator_quote.as_deref())
        .await?;
    // The enqueue may have promoted a previously-queued send (the idle-flush
    // safety net); broadcast so the browser sees the queued->dispatched
    // transition immediately.
    state.broadcast(events);
    Ok((StatusCode::CREATED, Json(WireSendResponse::from(send))))
}

/// `POST /api/sends/{id}/cancel` — cancel a queued or dispatched send (204).
///
/// A send composed while the assistant's turn is in flight is held in the
/// `queued` state until the session goes idle; this abandons such a send
/// before that dispatch. A `dispatched` send the turn machine is awaiting
/// (its echo has not arrived — typically the user pressed `Escape` in the
/// TUI to discard the composer buffer, leaving no signal Delta can observe)
/// is cancelled by injecting a single `Escape` keystroke into the pane and
/// dropping the row to `cancelled` — any send queued behind the cancelled
/// head then promotes through the existing idle-flush. A `dispatched` row
/// the turn machine holds no claim on is cancelled as a pure state
/// transition — no keystroke is injected and the turn machine is untouched.
/// The row flips to `cancelled` in every success case and drops out of the
/// open-send list (the browser refetches that list to clear the chip — no
/// event is broadcast).
///
/// Replies `409` with code `send_not_cancellable` when the send no longer
/// exists, is already terminal (matched a transcript line, or already
/// cancelled), or is `dispatched` but its echo has already arrived (the
/// turn carries it in flight; the user reaches for the in-flight interrupt
/// instead). The browser drops its cancel control and reconciles from the
/// refetch on this code.
pub(crate) async fn cancel_send(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().cancel_send(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sends/{id}/release` — release a *held* send into the normal
/// queued flow (204).
///
/// Two paths leave a row `queued` with a `held_at` marker — visible in the
/// open-send list, but never auto-dispatched: the boot-time reconcile,
/// recovering every send a dead server process left `dispatched`, and the
/// echo-deadline park, for a send whose keystrokes were swallowed without a
/// trace twice running.
///
/// This endpoint is the explicit "Send" action on such a row: it first ensures
/// the owning session is open (resuming `claude --resume <id>` when it is
/// closed, the normal state right after a restart), then clears the marker (a
/// guarded UPDATE, so a race with a cancel is a clean conflict) and runs the
/// session's normal queued dispatch — if the session was already open and idle
/// the row types immediately (the `send_dispatched` event is broadcast); if the
/// release resumed the session the row is typed by the resume-settle flush once
/// the resumed pane accepts input; otherwise (mid-turn) it waits as an ordinary
/// queued send for the turn-end trigger. The sibling Cancel action is the
/// existing [`cancel_send`] — a held row's status is still `queued`, so the
/// guarded queued cancel already covers it.
///
/// Replies `409` with code `send_not_releasable` when the send is unknown,
/// was never held, is already released, or has since been cancelled.
/// The browser drops its Send control and reconciles from the refetch on
/// this code. An ensure-open failure surfaces on its own path — e.g. `409`
/// `resume_unavailable` when the session's transcript is gone — before the
/// marker is touched, so the release can be retried.
pub(crate) async fn release_send(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let events = state.interactor().release_send(id).await?;
    // The release may have dispatched the released (or an older queued) send;
    // broadcast so the browser sees the queued→dispatched transition.
    state.broadcast(events);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/permissions/{id}/decision` — answer a pending tool-permission
/// request from the browser.
///
/// Resolves the request row and wakes the blocked `PermissionRequest` hook
/// response, which carries the decision back to Claude Code — so the tool
/// proceeds (or is denied) without anyone touching the TUI prompt. Replies
/// `409` when the request is no longer awaiting a browser decision (already
/// decided, or its hook wait timed out and the TUI prompt owns it now); the
/// browser then falls back to guidance chosen by the provider's `has_terminal`
/// capability.
pub(crate) async fn decide_permission(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WirePermissionDecisionRequest>,
) -> Result<StatusCode, ApiError> {
    let events = state
        .interactor()
        .decide_permission(id, req.decision.into())
        .await?;
    state.broadcast(events);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/questions/{request_id}/answer` — answer a pending
/// `AskUserQuestion` from the browser.
///
/// A CLI hook cannot return the user's pick, so the server turns the per-question
/// selected option indices into the exact TUI keystrokes (the pinned
/// key-sequence generator) and injects them into the session's live pane. The
/// TUI then records the answer and the turn proceeds; the eventual `tool_result`
/// resolves the question's request row through the normal sync, which clears the
/// card via the same `permission_resolved` path a terminal-answered question
/// takes — so no event is broadcast here.
///
/// Replies `409` when the question is no longer pending (already answered, its
/// turn ended, or no live pane) and `400` for a malformed selection; the browser
/// then falls back to the answer-in-the-terminal guidance.
pub(crate) async fn answer_question(
    State(state): State<AppState>,
    Path((id, request_id)): Path<(String, i64)>,
    Json(req): Json<WireQuestionAnswerRequest>,
) -> Result<StatusCode, ApiError> {
    // The wire form uses `u32` indices (non-negative on the wire); widen to the
    // `usize` the domain generator indexes options with.
    let selections: Vec<Vec<usize>> = req
        .selections
        .into_iter()
        .map(|group| group.into_iter().map(|index| index as usize).collect())
        .collect();
    state
        .interactor()
        .answer_question(&SessionId::from(id), request_id, selections)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/open-cwd` — launch an external tool at a session's cwd.
///
/// Currently only VS Code is registered as a handler (spawned as
/// `code <path>`), but the request already carries an optional `handler` id
/// so a future addition can drop in without a breaking change. Defaults to
/// the `vscode` handler when `handler` is absent.
///
/// The request `path` MUST be a path Delta has already surfaced to the
/// browser (a `session.cwd`, `session.requested_workdir`, or `message.cwd`).
/// The interactor checks this allowlist before invoking the opener, so a
/// hand-crafted request cannot point the editor at an arbitrary directory
/// on disk. Replies `204 No Content` on a successful spawn; the browser
/// shows no toast on success — the editor opening is self-evident.
///
/// Error responses:
///
/// - `400` with code `open_cwd_path_not_allowed` — the path is not in the
///   allowlist. The click site never sends one, so this only fires against
///   a hand-crafted request.
/// - `400` with code `open_cwd_unknown_handler` — the `handler` id is not
///   registered. Same UX as above.
/// - `500` with code `open_cwd_command_not_found` — the tool binary
///   (`code`) is not installed on the server host. The browser renders a
///   specific "VS Code is not installed" message.
/// - `500` with code `open_cwd_spawn_failed` — any other spawn failure
///   (fork error, permission denied, etc.).
pub(crate) async fn open_cwd(
    State(state): State<AppState>,
    Json(req): Json<WireOpenCwdRequest>,
) -> Result<StatusCode, ApiError> {
    let path = req.path.trim();
    if path.is_empty() {
        return Err(ApiError::BadRequest(
            "`path` must be a non-blank string".to_owned(),
        ));
    }
    state
        .interactor()
        .open_cwd(path, req.handler.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/version` — the Delta workspace version, pre-formatted for the
/// browser footer.
///
/// Owned by the server (not the browser) so the format contract lives in one
/// place: release builds return `v<version>`, debug builds return
/// `v<version>+dev.<short-sha>`. See `crate::version::display_version` for the
/// rationale on `+dev` (SemVer build metadata) vs `-dev` (pre-release).
pub(crate) async fn get_version() -> Json<WireVersionResponse> {
    Json(WireVersionResponse {
        version: crate::version::display_version(),
    })
}

/// `POST /api/sessions/{id}/questions/cancel` — cancel a pending
/// `AskUserQuestion` from the browser.
///
/// The sibling of [`answer_question`]: a CLI hook cannot cancel the question, so
/// the server injects a single `Escape` into the session's live pane, which
/// cancels the whole call. The TUI then writes an `is_error` `tool_result`, and
/// that flush resolves the question's request row through the normal sync, which
/// clears the card via the same `permission_resolved` path a terminal-cancelled
/// question takes — so no event is broadcast here.
///
/// Unlike an answer, cancel carries no selection, so the `request_id` rides in
/// the body rather than the path. Replies `409` when the question is no longer
/// pending (already answered/cancelled, its turn ended, or no live pane); the
/// browser then falls back to the cancel-in-the-terminal guidance. There is no
/// `400` case — there is no selection to malform.
pub(crate) async fn cancel_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<WireQuestionCancelRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .interactor()
        .cancel_question(&SessionId::from(id), req.request_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `require_path` hands its callers a trimmed path. Asserting on the
    /// returned slice pins that directly, without a gateway stub whose only job
    /// would be to record the string it received.
    #[test]
    fn require_path_returns_a_trimmed_path() {
        let query = WorkdirGitQuery {
            path: Some("  /projects/app  ".to_owned()),
        };
        assert_eq!(query.require_path().ok(), Some("/projects/app"));
    }

    /// Missing, empty, and whitespace-only `path` values are all "blank" as far
    /// as the documented contract is concerned.
    #[test]
    fn require_path_rejects_a_blank_path() {
        for path in [None, Some(String::new()), Some("   ".to_owned())] {
            let query = WorkdirGitQuery { path: path.clone() };
            assert!(
                matches!(query.require_path(), Err(ApiError::BadRequest(_))),
                "expected a 400 for {path:?}",
            );
        }
    }
}
