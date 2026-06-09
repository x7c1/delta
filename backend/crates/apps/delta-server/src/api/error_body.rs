//! The JSON body returned for any error response.

use serde::Serialize;

/// The JSON body returned for any error response.
///
/// `error` is a human-readable message. `code` is an optional stable,
/// machine-readable identifier the frontend can branch on (e.g.
/// `"resume_unavailable"`); it is omitted for errors that carry no distinct
/// code, keeping those responses unchanged.
#[derive(Debug, Serialize)]
pub(crate) struct ErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
}
