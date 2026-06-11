//! Crate-local error type for the composition root.

use thiserror::Error;

/// Errors raised while wiring the application together.
#[derive(Debug, Error)]
pub enum Error {
    /// The session store could not be opened.
    #[error("failed to open store: {0}")]
    Store(#[from] delta_sqlite::Error),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
