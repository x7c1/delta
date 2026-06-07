//! A use-case error rendered as an HTTP response.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::error_body::ErrorBody;

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
        let (status, message) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::UseCase(err) => {
                let status = match &err {
                    // No session yet means nothing to act on for the caller.
                    Error::NoSession => StatusCode::NOT_FOUND,
                    Error::ThreadNotFound(_) | Error::SessionNotFound(_) => StatusCode::NOT_FOUND,
                    // Everything else is an internal failure.
                    Error::Tmux(_)
                    | Error::Transcript(_)
                    | Error::Store(_)
                    | Error::Workspace(_) => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, err.to_string())
            }
        };
        if status.is_server_error() {
            tracing::error!(error = %message, "api handler failed");
        }
        (status, Json(ErrorBody { error: message })).into_response()
    }
}
