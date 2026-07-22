//! A use-case error rendered as an HTTP response.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use delta_wire::rest::WireErrorBody;

/// Stable machine-readable code for a resume-impossible session, carried in the
/// error body so the frontend can distinguish it from a generic failure.
const RESUME_UNAVAILABLE_CODE: &str = "resume_unavailable";

/// Stable machine-readable code for a permission decision that can no longer
/// take effect (already decided, or its hook wait timed out and fell back to
/// the TUI prompt). The frontend switches the notice to the
/// answer-in-the-terminal guidance on this code.
const PERMISSION_NOT_PENDING_CODE: &str = "permission_not_pending";

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
/// never existed, was never restored by the boot-time reconcile, was already
/// released, or has since been cancelled). The frontend drops its Send control
/// and reconciles its pending strip from the next refetch on this code.
const SEND_NOT_RELEASABLE_CODE: &str = "send_not_releasable";

/// Stable machine-readable code for a repository scan root registered twice
/// with the same path. The Settings dialog shows an inline "already registered"
/// hint instead of a generic failure toast on this code.
const SCAN_ROOT_DUPLICATE_CODE: &str = "scan_root_duplicate";

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
                    // The permission request exists (or existed) but no browser
                    // decision can reach it anymore: a conflict with current
                    // state, with a stable code so the frontend swaps the
                    // Allow/Deny buttons for the answer-in-the-terminal
                    // guidance.
                    Error::PermissionNotPending(_) => {
                        (StatusCode::CONFLICT, Some(PERMISSION_NOT_PENDING_CODE))
                    }
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
                    // The send is not a still-queued restored row, so a
                    // release can no longer take effect: a conflict with
                    // current state, with a stable code so the frontend drops
                    // the Send control and reconciles from the next refetch.
                    Error::SendNotReleasable(_) => {
                        (StatusCode::CONFLICT, Some(SEND_NOT_RELEASABLE_CODE))
                    }
                    // A repository scan root registered twice: a conflict with
                    // current state, with a stable code so the Settings dialog
                    // shows an inline hint instead of a generic failure toast.
                    Error::RepositoryScanRootDuplicate(_) => {
                        (StatusCode::CONFLICT, Some(SCAN_ROOT_DUPLICATE_CODE))
                    }
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
                    // Everything else is an internal failure.
                    Error::Tmux(_)
                    | Error::Agent(_)
                    | Error::Git(_)
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
