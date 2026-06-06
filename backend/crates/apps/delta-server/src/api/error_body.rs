//! The JSON body returned for any error response.

use serde::Serialize;

/// The JSON body returned for any error response.
#[derive(Debug, Serialize)]
pub(crate) struct ErrorBody {
    pub error: String,
}
