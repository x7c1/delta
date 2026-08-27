//! A use-case error rendered as an HTTP response.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use delta_wire::rest::WireErrorBody;

/// Stable machine-readable code for a resume-impossible session, carried in the
/// error body so the frontend can distinguish it from a generic failure.
const RESUME_UNAVAILABLE_CODE: &str = "resume_unavailable";

/// Stable machine-readable code for a **branch** send aimed at a session whose
/// launch has not bound yet. Such a session is listed (and focusable) from the
/// moment its first send is accepted, so its composer is reachable while it is
/// still starting — and a plain send there is accepted as a `queued` row rather
/// than refused. A branch send is the one shape with nowhere to go: the session
/// has ingested no message to branch from. The browser cannot compose one
/// either, for the same reason (branching anchors on a message), so no frontend
/// path words this code today; it keeps the case distinguishable for an API
/// client, and for a browser that grows the path later.
const SESSION_SPAWNING_CODE: &str = "session_spawning";

/// Stable machine-readable code for a permission decision that can no longer
/// take effect (already decided, or its hook wait timed out and fell back to
/// the TUI prompt). The frontend switches the notice to guidance chosen by the
/// provider's `has_terminal` capability on this code.
const PERMISSION_NOT_PENDING_CODE: &str = "permission_not_pending";

/// Stable machine-readable code for a permission decision whose *value* the
/// session's provider cannot express — today a session-scoped allow against a
/// provider that does not declare the capability. Distinct from
/// [`PERMISSION_NOT_PENDING_CODE`] and a `400` rather than a `409`: the request
/// is still pending and a plain allow or deny would be honoured, so the frontend
/// keeps the decision buttons rather than switching to a fallback.
const PERMISSION_DECISION_UNSUPPORTED_CODE: &str = "permission_decision_unsupported";

/// Stable machine-readable code for an answer to a question that is no longer
/// pending (already answered, its turn ended, or no live pane). The frontend
/// switches the card to the answer-in-the-terminal fallback on this code.
const QUESTION_NOT_PENDING_CODE: &str = "question_not_pending";

/// Stable machine-readable code for a send that can no longer be cancelled (it
/// never existed, is already terminal, or its echo has already arrived). The
/// frontend drops its cancel control and reconciles its pending strip from the
/// next refetch on this code.
const SEND_NOT_CANCELLABLE_CODE: &str = "send_not_cancellable";

/// Stable machine-readable code for a send that is not awaiting a release (it
/// never existed, was never held, was already released, or has since been
/// cancelled). The frontend drops its Send control and reconciles its pending
/// strip from the next refetch on this code.
const SEND_NOT_RELEASABLE_CODE: &str = "send_not_releasable";

/// Stable machine-readable code for a clone root registered twice with the same
/// path. The Settings dialog shows an inline "already registered" hint instead
/// of a generic failure toast on this code.
const CLONE_ROOT_DUPLICATE_CODE: &str = "clone_root_duplicate";

/// Stable machine-readable code for a clone requested into a directory that is
/// not a registered clone root. The PR tab's inline clone panel shows the
/// message on the row that asked for the clone rather than as a toast; the code
/// is what keeps this refusal identifiable to a client that wants to word it
/// itself, instead of an anonymous `400`.
const CLONE_ROOT_NOT_REGISTERED_CODE: &str = "clone_root_not_registered";

/// Stable machine-readable code for a delete aimed at a launch option Delta
/// ships. The Settings list renders no delete control on such a row, so a
/// client only meets this from a stale list.
const LAUNCH_OPTION_BUILTIN_CODE: &str = "launch_option_builtin";

/// Stable machine-readable code for a session-start selection the provider's
/// adapter will not apply (a Delta-owned field, the same field twice, or two
/// selected Codex `config` rows that disagree about one setting). The message is
/// the only thing that says *which* selection is wrong — it names the offending
/// key, or every conflicting key path — so the frontend shows it verbatim on the
/// failed send rather than a generic "could not be sent", and the code is what
/// tells it this `400` carries such a message at all.
const LAUNCH_OPTION_REJECTED_CODE: &str = "launch_option_rejected";

/// Stable machine-readable code for a clone whose destination
/// (`<clone_root>/<repo_name>`) already exists. The clone panel shows the
/// message inline on the row; there is no fallback naming, so the way past it
/// is a different clone root, not a retry.
const CLONE_DEST_EXISTS_CODE: &str = "clone_dest_exists";

/// Stable machine-readable code for a `POST /api/open-cwd` request whose
/// `path` is not in the known-cwd allowlist. The frontend surfaces the
/// generic "opening failed" message on this code — the click site should
/// never send an unknown path, so this only fires against a hand-crafted
/// request that the user should not see.
const OPEN_CWD_PATH_NOT_ALLOWED_CODE: &str = "open_cwd_path_not_allowed";

/// Stable machine-readable code for an unknown `handler` id in
/// `POST /api/open-cwd`. Same UX as
/// [`OPEN_CWD_PATH_NOT_ALLOWED_CODE`] — it should never fire on the happy
/// path.
const OPEN_CWD_UNKNOWN_HANDLER_CODE: &str = "open_cwd_unknown_handler";

/// Stable machine-readable code for `code` (or a future handler's command)
/// missing on `PATH`. The frontend renders a specific "VS Code is not
/// installed" message on this code so the user has a clear next step
/// (install the shell `code` command) instead of a generic failure.
const OPEN_CWD_COMMAND_NOT_FOUND_CODE: &str = "open_cwd_command_not_found";

/// Stable machine-readable code for a spawn failure that is *not* a missing
/// binary (fork failure, permission denied, etc.). The frontend renders the
/// generic error message on this code.
const OPEN_CWD_SPAWN_FAILED_CODE: &str = "open_cwd_spawn_failed";

/// An error rendered as an HTTP response.
///
/// This is the single place that maps failures onto status codes, keeping the
/// handlers free of ad-hoc error handling. It carries either a use-case
/// [`delta_usecase::Error`] or a request-shape rejection the schema alone cannot
/// express (a `400`).
pub(crate) enum ApiError {
    /// A failure raised while executing a use case.
    UseCase(delta_usecase::Error),
    /// A malformed request rejected before any use case runs (`400`).
    BadRequest(String),
    /// A request targeting a resource that does not exist (`404`), where the
    /// use case reports absence as `None` rather than an [`delta_usecase::Error`].
    NotFound(String),
}

impl From<delta_usecase::Error> for ApiError {
    fn from(err: delta_usecase::Error) -> Self {
        ApiError::UseCase(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        use delta_usecase::Error;
        let (status, message, code) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message, None),
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message, None),
            ApiError::UseCase(err) => {
                let (status, code) = match &err {
                    // No session yet means nothing to act on for the caller.
                    Error::NoSession => (StatusCode::NOT_FOUND, None),
                    Error::ThreadNotFound(_) | Error::SessionNotFound(_) => {
                        (StatusCode::NOT_FOUND, None)
                    }
                    // The session exists but its transcript is gone, so resume is
                    // impossible. This is a conflict with current state, not a
                    // server fault: report `409` with a stable code so the
                    // frontend can keep the session closed and show a specific
                    // "cannot be resumed" message instead of a generic failure.
                    Error::ResumeUnavailable(_) => {
                        (StatusCode::CONFLICT, Some(RESUME_UNAVAILABLE_CODE))
                    }
                    // The session exists but its launch has not bound yet, so
                    // it has no message to branch from: a conflict with current
                    // state (the same branch send succeeds once the launch
                    // registers), reported with a stable code so a client can
                    // tell it apart from a generic failure.
                    Error::SessionSpawning(_) => {
                        (StatusCode::CONFLICT, Some(SESSION_SPAWNING_CODE))
                    }
                    // The permission request exists (or existed) but no browser
                    // decision can reach it anymore: a conflict with current
                    // state, with a stable code so the frontend swaps the
                    // Allow/Deny buttons for guidance chosen by the provider's
                    // `has_terminal` capability.
                    Error::PermissionNotPending(_) => {
                        (StatusCode::CONFLICT, Some(PERMISSION_NOT_PENDING_CODE))
                    }
                    // The request is still pending; it is the decision *value*
                    // this session's provider has no meaning for (a
                    // session-scoped allow where the capability is not
                    // declared). A malformed request, not a state conflict, so
                    // `400` — and nothing was mutated, so the same request is
                    // still answerable with a decision the provider does have.
                    Error::PermissionDecisionUnsupported(_) => (
                        StatusCode::BAD_REQUEST,
                        Some(PERMISSION_DECISION_UNSUPPORTED_CODE),
                    ),
                    // The question exists (or existed) but cannot be answered
                    // from the UI anymore: a conflict with current state, with a
                    // stable code so the frontend keeps the terminal fallback.
                    Error::QuestionNotPending(_) => {
                        (StatusCode::CONFLICT, Some(QUESTION_NOT_PENDING_CODE))
                    }
                    // The send is unknown, already terminal, or its echo has
                    // already arrived, so a cancel can no longer take effect: a
                    // conflict with current state, with a stable code so the
                    // frontend drops the cancel control and reconciles from the
                    // next refetch.
                    Error::SendNotCancellable(_) => {
                        (StatusCode::CONFLICT, Some(SEND_NOT_CANCELLABLE_CODE))
                    }
                    // The send is not a still-queued held row, so a release
                    // can no longer take effect: a conflict with current
                    // state, with a stable code so the frontend drops the
                    // Send control and reconciles from the next refetch.
                    Error::SendNotReleasable(_) => {
                        (StatusCode::CONFLICT, Some(SEND_NOT_RELEASABLE_CODE))
                    }
                    // A clone root registered twice: a conflict with current
                    // state, with a stable code so the Settings dialog shows an
                    // inline hint instead of a generic failure toast.
                    Error::CloneRootDuplicate(_) => {
                        (StatusCode::CONFLICT, Some(CLONE_ROOT_DUPLICATE_CODE))
                    }
                    // A clone aimed somewhere the user never registered as a
                    // home for clones. The caller can fix it (register the root,
                    // or pick a registered one), so `400` with a stable code so
                    // the clone panel can say which of the two it is.
                    Error::CloneRootNotRegistered(_) => (
                        StatusCode::BAD_REQUEST,
                        Some(CLONE_ROOT_NOT_REGISTERED_CODE),
                    ),
                    // Something already occupies the one path this clone could
                    // land on: a conflict with the state of the filesystem, not
                    // a malformed request, so `409` — with a stable code so the
                    // row shows the specific reason instead of a generic error.
                    Error::CloneDestinationExists(_) => {
                        (StatusCode::CONFLICT, Some(CLONE_DEST_EXISTS_CODE))
                    }
                    // An owner/name that cannot be one path component. The click
                    // site never produces one (these come from gh's own PR rows),
                    // so this only fires against a hand-crafted request: `400`
                    // with no code, since no UI branches on it.
                    Error::InvalidRepositoryRef(_) => (StatusCode::BAD_REQUEST, None),
                    // The browser's selection could not be turned into a key
                    // sequence (malformed, or an unsupported sub-case): the
                    // caller sent a bad answer, so `400`.
                    Error::InvalidQuestionAnswer(_) => (StatusCode::BAD_REQUEST, None),
                    // A user-selected directory that does not exist or is not a
                    // directory is a client error: the caller named a bad path.
                    Error::InvalidWorkdir(_) => (StatusCode::BAD_REQUEST, None),
                    // The path exists but the server cannot read it: distinct
                    // from "bad path", so report `403` rather than `400`.
                    Error::WorkdirPermission(_) => (StatusCode::FORBIDDEN, None),
                    // A worktree was requested for a directory that is not a git
                    // repo, or with no directory at all: the caller's request
                    // shape is invalid, so `400`.
                    Error::WorktreeNotAGitRepo(_) | Error::WorktreeRequiresWorkdir => {
                        (StatusCode::BAD_REQUEST, None)
                    }
                    // A path the server has never shown the browser is a
                    // client error (the click site never sends one), but it is
                    // surfaced with a stable code so the browser can
                    // distinguish it from a generic 400.
                    Error::OpenCwdPathNotAllowed(_) => (
                        StatusCode::BAD_REQUEST,
                        Some(OPEN_CWD_PATH_NOT_ALLOWED_CODE),
                    ),
                    // An unknown handler id is also a client-side bug:
                    // the initial impl only registers `vscode`.
                    Error::OpenCwdUnknownHandler(_) => {
                        (StatusCode::BAD_REQUEST, Some(OPEN_CWD_UNKNOWN_HANDLER_CODE))
                    }
                    // The external tool is not installed: a configuration issue
                    // on the user's machine (fix: install VS Code's shell
                    // `code` command). 500 + a stable code so the browser
                    // shows the specific "not installed" message.
                    Error::ExternalOpenerCommandNotFound(_) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Some(OPEN_CWD_COMMAND_NOT_FOUND_CODE),
                    ),
                    // Any other spawn failure (fork, permission denied):
                    // 500 with a stable code so the browser can pick a
                    // less specific message.
                    Error::ExternalOpenerSpawnFailed(_) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Some(OPEN_CWD_SPAWN_FAILED_CODE),
                    ),
                    // A template with a blank label or blank body: the caller
                    // can fix it, so 400 with the message naming the offending
                    // field.
                    Error::InvalidPromptTemplate(_) => (StatusCode::BAD_REQUEST, None),
                    // A selected launch option the provider's adapter will not
                    // apply (a Delta-owned field, the same field twice, or two
                    // selected Codex `config` rows disagreeing about one
                    // setting): the caller can fix it, so 400 with the
                    // adapter's message naming the offending key path(s), and a
                    // stable code so the frontend knows to show that message
                    // instead of its generic failure copy.
                    Error::LaunchOptionRejected(_) => {
                        (StatusCode::BAD_REQUEST, Some(LAUNCH_OPTION_REJECTED_CODE))
                    }
                    // A delete aimed at a launch option Delta ships. A 409,
                    // not a 400: the id is fine and the same call against a
                    // user row is honoured — it is the target's state that
                    // forbids the delete.
                    Error::LaunchOptionIsBuiltin(_) => {
                        (StatusCode::CONFLICT, Some(LAUNCH_OPTION_BUILTIN_CODE))
                    }
                    // Everything else is an internal failure. The two launch
                    // preparation failures only land here defensively: both
                    // happen long after the send was accepted, so they reach
                    // the browser as a `spawn_failed` event, never as a
                    // response body.
                    Error::Tmux(_)
                    | Error::Agent(_)
                    | Error::Git(_)
                    | Error::LaunchPreparationTimedOut(_)
                    | Error::WorktreeLandedElsewhere { .. }
                    | Error::Gh(_)
                    | Error::Transcript(_)
                    | Error::Store(_)
                    | Error::Workspace(_)
                    | Error::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, None),
                };
                (status, err.to_string(), code)
            }
        };
        if status.is_server_error() {
            tracing::error!(error = %message, "api handler failed");
        }
        (
            status,
            Json(WireErrorBody {
                error: message,
                code: code.map(str::to_owned),
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// Render an error through the response mapping and read back the status
    /// plus the body's stable `code`, which is the contract a client branches
    /// on (see `docs/guides/api/sends.md`).
    async fn rendered(err: delta_usecase::Error) -> (StatusCode, Option<String>) {
        let response = ApiError::from(err).into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let code = body
            .get("code")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        (status, code)
    }

    /// A branch send to a session whose launch has not bound yet is a conflict
    /// with current state, not a server fault: `409` with the stable
    /// `session_spawning` code, which keeps it distinguishable from the other
    /// send-time conflict. (A plain send there is queued, never refused.)
    #[tokio::test]
    async fn a_still_spawning_session_renders_a_conflict_with_its_code() {
        let (status, code) = rendered(delta_usecase::Error::SessionSpawning("sess-1".into())).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(code.as_deref(), Some("session_spawning"));

        // Distinct from the other send-time conflict, so a client can tell
        // "still starting" (retry once it registers) from "cannot be resumed".
        let (status, resume_code) =
            rendered(delta_usecase::Error::ResumeUnavailable("sess-2".into())).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(resume_code.as_deref(), Some("resume_unavailable"));
    }
}
