//! Crate-local error type for the SQLite gateway.

use thiserror::Error;

/// Errors raised by [`crate::SqliteStore`].
#[derive(Debug, Error)]
pub enum Error {
    /// A `rusqlite` failure.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A stored value did not parse back into a domain type.
    #[error("invalid stored value: {0}")]
    Decode(#[from] delta_model::Error),

    /// An expected row was missing.
    #[error("not found: {0}")]
    NotFound(String),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Store(value.to_string())
    }
}
