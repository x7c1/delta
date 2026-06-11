//! A use-case error rendered as an HTTP response.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use delta_wire::rest::WireErrorBody;

/// Stable machine-readable code for a resume-impossible session, carried in the
/// error body so the frontend can distinguish it from a generic failure.
const RESUME_UNAVAILABLE_CODE: &str = "resume_unavailable";

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
                    // A user-selected directory that does not exist or is not a
                    // directory is a client error: the caller named a bad path.
                    Error::InvalidWorkdir(_) => (StatusCode::BAD_REQUEST, None),
                    // The path exists but the server cannot read it: distinct
                    // from "bad path", so report `403` rather than `400`.
                    Error::WorkdirPermission(_) => (StatusCode::FORBIDDEN, None),
                    // Everything else is an internal failure.
                    Error::Tmux(_)
                    | Error::Transcript(_)
                    | Error::Store(_)
                    | Error::Workspace(_) => (StatusCode::INTERNAL_SERVER_ERROR, None),
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
