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

    /// The on-disk schema generation (`PRAGMA user_version`) does not match the
    /// binary's expected [`crate::SCHEMA_VERSION`]. Raised by the startup gate
    /// in [`crate::SqliteStore::init`] so the server exits cleanly with a
    /// `make reset` hint, instead of letting the mismatch surface later as
    /// confusing runtime errors.
    #[error(
        "delta SQLite overlay schema version mismatch: \
         database is at version {found}, this binary expects version {expected}. \
         Run `make reset` to rebuild the overlay."
    )]
    SchemaMismatch {
        /// The version stored in the on-disk file's `PRAGMA user_version`.
        found: u32,
        /// The version this binary was built against ([`crate::SCHEMA_VERSION`]).
        expected: u32,
    },
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Store(value.to_string())
    }
}
