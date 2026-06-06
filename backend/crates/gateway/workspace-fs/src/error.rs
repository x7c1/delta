//! Crate-local error type for the filesystem workspace gateway.

use thiserror::Error;

/// Errors raised by [`crate::FsWorkspace`].
#[derive(Debug, Error)]
pub enum Error {
    /// The working directory or settings file could not be created or written.
    #[error("workspace io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Workspace(value.to_string())
    }
}
