//! Crate-local error type for the transcript gateway.

use thiserror::Error;

/// Errors raised by [`crate::JsonlTranscript`].
#[derive(Debug, Error)]
pub enum Error {
    /// The transcript file could not be read.
    #[error("transcript io error: {0}")]
    Io(#[from] std::io::Error),

    /// A line could not be parsed as JSON.
    #[error("transcript parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Transcript(value.to_string())
    }
}
