//! Why a line could not be turned into an [`Incoming`].

#[cfg(doc)]
use super::Incoming;

/// Why a line could not be turned into an [`Incoming`].
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The line was not valid JSON.
    #[error("frame is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// The line was valid JSON but not a recognisable JSON-RPC frame (e.g. no
    /// `method` and no `result`/`error`), or an id that is not an integer on a
    /// response.
    #[error("frame is not a recognisable JSON-RPC message: {0}")]
    NotRpc(String),
}
