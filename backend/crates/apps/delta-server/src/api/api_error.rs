//! A use-case error rendered as an HTTP response.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::error_body::ErrorBody;

/// A use-case error rendered as an HTTP response.
///
/// This is the single place that maps [`delta_usecase::Error`] onto status
/// codes, keeping the handlers free of ad-hoc error handling.
pub(crate) struct ApiError(delta_usecase::Error);

impl From<delta_usecase::Error> for ApiError {
    fn from(err: delta_usecase::Error) -> Self {
        ApiError(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        use delta_usecase::Error;
        let status = match &self.0 {
            // No session yet means nothing to act on for the caller.
            Error::NoSession => StatusCode::NOT_FOUND,
            Error::ThreadNotFound(_) => StatusCode::NOT_FOUND,
            // Everything else is an internal failure.
            Error::Tmux(_) | Error::Transcript(_) | Error::Store(_) | Error::Workspace(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        if status.is_server_error() {
            tracing::error!(error = %self.0, "api handler failed");
        }
        let body = Json(ErrorBody {
            error: self.0.to_string(),
        });
        (status, body).into_response()
    }
}
